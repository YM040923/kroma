//! Provider-neutral acquisition planning: owned check first, then the next
//! action and the provider order. No network calls live here; the server calls
//! the module points once this plan says an acquisition is needed.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use kroma_module_host::HostStorage;
use kroma_primitives::now_ms;

use crate::db;
use crate::model::{
    AcquisitionAttempt, AcquisitionPolicy, MediaUnit, MediaUnitKind, MediaUnitRef, OwnedState,
};

pub mod owned;

pub use owned::{locations, owned};

/// What should happen next for a unit, independent of provider implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    /// Both required locations are present.
    Complete,
    /// Local bytes exist; only the cloud replica is missing.
    UploadToCloud,
    /// Cloud bytes exist; only the local copy is missing.
    DownloadToLocal,
    /// Cloud bytes exist and the staging file is no longer needed.
    EvictLocalStaging,
    /// Nothing usable exists; ask providers in order.
    Acquire,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionPlan {
    pub unit_key: String,
    pub policy: AcquisitionPolicy,
    pub owned: OwnedState,
    pub next: NextAction,
    /// Provider order to try when `next == Acquire`; empty for placement work.
    pub providers: Vec<String>,
    pub active_attempt: Option<AcquisitionAttempt>,
}

/// Decide placement before acquisition. A cloud-only staged library tries the
/// cloud first and uses BT only through a local staging file; a hybrid library
/// downloads locally first and uploads the replica afterwards.
pub fn plan<S: HostStorage>(
    state: &S,
    unit: &MediaUnitRef,
    policy: AcquisitionPolicy,
) -> Result<AcquisitionPlan> {
    let owned = owned(state, unit)?;
    let next = next_action(policy, owned);
    let conn = state.db().get()?;
    let active_attempt = db::active_attempt(&conn, &unit.key())?;
    let providers = if next == NextAction::Acquire {
        policy
            .provider_order()
            .iter()
            .map(|provider| (*provider).to_string())
            .collect()
    } else {
        Vec::new()
    };
    Ok(AcquisitionPlan {
        unit_key: unit.key(),
        policy,
        owned,
        next,
        providers,
        active_attempt,
    })
}

pub fn next_action(policy: AcquisitionPolicy, owned: OwnedState) -> NextAction {
    if policy.requires_cloud() && !owned.cloud {
        return if owned.local {
            NextAction::UploadToCloud
        } else {
            NextAction::Acquire
        };
    }
    if policy.requires_local() && !owned.local {
        return if owned.cloud {
            NextAction::DownloadToLocal
        } else {
            NextAction::Acquire
        };
    }
    if policy.evict_after_cloud_verified && owned.cloud && owned.local {
        return NextAction::EvictLocalStaging;
    }
    NextAction::Complete
}

/// Materialize the identity row before an attempt, so attempts and locations
/// never dangle from a unit that only lives in an acquisition response.
pub fn ensure_unit<S: HostStorage>(
    state: &S,
    unit: &MediaUnitRef,
    title: &str,
    year: Option<u32>,
) -> Result<MediaUnit> {
    if !unit.valid() {
        anyhow::bail!("invalid media unit identity");
    }
    let conn = state.db().get()?;
    db::upsert_media_unit(&conn, unit, title, year, now_ms())
}

/// A movie or episode reference from a request-shaped title. Kept here so the
/// UI/API layer does not have to know the key format.
pub fn movie_unit(tmdb_id: u64) -> MediaUnitRef {
    MediaUnitRef::movie(tmdb_id)
}

pub fn episode_unit(tmdb_id: u64, season: u32, episode: u32) -> MediaUnitRef {
    MediaUnitRef::episode(tmdb_id, season, episode)
}

pub fn is_movie(unit: &MediaUnitRef) -> bool {
    matches!(unit.kind, MediaUnitKind::Movie)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kroma_module_host::testing::StubHost;

    fn unit() -> MediaUnitRef {
        MediaUnitRef::movie(603)
    }

    #[test]
    fn hybrid_with_neither_side_acquires_then_fills_both() {
        let policy = AcquisitionPolicy::hybrid_local_first();
        assert_eq!(
            next_action(policy, OwnedState::default()),
            NextAction::Acquire
        );
        assert_eq!(
            next_action(
                policy,
                OwnedState {
                    local: true,
                    cloud: false
                }
            ),
            NextAction::UploadToCloud
        );
        assert_eq!(
            next_action(
                policy,
                OwnedState {
                    local: false,
                    cloud: true
                }
            ),
            NextAction::DownloadToLocal
        );
        assert_eq!(
            next_action(
                policy,
                OwnedState {
                    local: true,
                    cloud: true
                }
            ),
            NextAction::Complete
        );
    }

    #[test]
    fn cloud_only_staged_falls_back_to_bt_and_evicts_after_upload() {
        let policy = AcquisitionPolicy::cloud_only_staged();
        assert_eq!(
            next_action(policy, OwnedState::default()),
            NextAction::Acquire
        );
        assert_eq!(policy.provider_order(), &["cloud115", "bt"]);
        assert_eq!(
            next_action(
                policy,
                OwnedState {
                    local: true,
                    cloud: false
                }
            ),
            NextAction::UploadToCloud
        );
        assert_eq!(
            next_action(
                policy,
                OwnedState {
                    local: true,
                    cloud: true
                }
            ),
            NextAction::EvictLocalStaging
        );
        assert_eq!(
            next_action(
                policy,
                OwnedState {
                    local: false,
                    cloud: true
                }
            ),
            NextAction::Complete
        );
    }

    #[test]
    fn pure_cloud_only_has_no_bt_fallback() {
        let policy = AcquisitionPolicy {
            placement: crate::model::Placement::CloudOnly,
            order: crate::model::AcquisitionOrder::CloudFirst,
            local_staging: false,
            evict_after_cloud_verified: false,
        };
        assert_eq!(policy.provider_order(), &["cloud115"]);
        assert_eq!(
            next_action(policy, OwnedState::default()),
            NextAction::Acquire
        );
    }

    #[test]
    fn local_only_does_not_require_cloud() {
        assert_eq!(
            next_action(
                AcquisitionPolicy::local_only(),
                OwnedState {
                    local: true,
                    cloud: false
                }
            ),
            NextAction::Complete
        );
        assert_eq!(
            next_action(
                AcquisitionPolicy::local_only(),
                OwnedState {
                    local: false,
                    cloud: true
                }
            ),
            NextAction::DownloadToLocal
        );
    }

    #[test]
    fn a_cloud_location_makes_a_unit_owned_without_local_bytes() {
        let host = StubHost::with_core(crate::db::testing::temp_pool("acquisition-owned"));
        let unit = unit();
        let conn = host.db().get().unwrap();
        crate::db::upsert_media_unit(&conn, &unit, "Film", Some(1999), 1).unwrap();
        crate::db::upsert_media_location(
            &conn,
            &crate::model::MediaLocation {
                id: "cloud".into(),
                unit_key: unit.key(),
                provider: crate::model::PROVIDER_CLOUD115.into(),
                account_id: "a".into(),
                path: "/Movies/Film.mkv".into(),
                remote_id: "pc".into(),
                origin: "imported".into(),
                state: crate::model::LocationState::Present,
                is_primary: false,
                size: None,
                fingerprint: None,
                quality: None,
                verified_at: Some(1),
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        drop(conn);
        assert_eq!(
            owned(&host, &unit).unwrap(),
            OwnedState {
                local: false,
                cloud: true
            }
        );
        let plan = plan(&host, &unit, AcquisitionPolicy::hybrid_local_first()).unwrap();
        assert_eq!(plan.next, NextAction::DownloadToLocal);
        assert!(plan.providers.is_empty());
    }

    #[test]
    fn a_staged_cloud_library_acquires_cloud_first() {
        let host = StubHost::with_core(crate::db::testing::temp_pool("acquisition-staged"));
        let plan = plan(&host, &unit(), AcquisitionPolicy::cloud_only_staged()).unwrap();
        assert_eq!(plan.next, NextAction::Acquire);
        assert_eq!(plan.providers, vec!["cloud115", "bt"]);
    }
}
