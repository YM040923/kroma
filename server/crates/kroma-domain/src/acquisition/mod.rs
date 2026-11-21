//! The acquisition ledger: one logical unit, where its bytes live, and the
//! provider attempts that tried to bring it here.
//!
//! Requests and wanted rows remain the demand layer. These types are the
//! identity and placement layer shared by every provider, so BT and 115 can
//! answer "do we already have this?" against the same facts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROVIDER_LOCAL: &str = "local";
pub const PROVIDER_BT: &str = "bt";
pub const PROVIDER_CLOUD115: &str = "cloud115";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaUnitKind {
    Movie,
    Episode,
}

impl MediaUnitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaUnitKind::Movie => "movie",
            MediaUnitKind::Episode => "episode",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "movie" => Some(MediaUnitKind::Movie),
            "episode" => Some(MediaUnitKind::Episode),
            _ => None,
        }
    }
}

/// The stable identity of one thing a viewer wants. A movie uses season=episode=0;
/// an episode is one (show tmdb id, season, episode) triple.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaUnitRef {
    pub kind: MediaUnitKind,
    pub tmdb_id: u64,
    pub season: u32,
    pub episode: u32,
}

impl MediaUnitRef {
    pub fn movie(tmdb_id: u64) -> Self {
        Self {
            kind: MediaUnitKind::Movie,
            tmdb_id,
            season: 0,
            episode: 0,
        }
    }

    pub fn episode(tmdb_id: u64, season: u32, episode: u32) -> Self {
        Self {
            kind: MediaUnitKind::Episode,
            tmdb_id,
            season,
            episode,
        }
    }

    pub fn key(&self) -> String {
        match self.kind {
            MediaUnitKind::Movie => format!("movie:{}", self.tmdb_id),
            MediaUnitKind::Episode => {
                format!("episode:{}:{}:{}", self.tmdb_id, self.season, self.episode)
            }
        }
    }

    pub fn valid(&self) -> bool {
        self.tmdb_id > 0
            && match self.kind {
                MediaUnitKind::Movie => self.season == 0 && self.episode == 0,
                MediaUnitKind::Episode => self.season > 0 && self.episode > 0,
            }
    }
}

/// One row of `media_units`, with the metadata snapshotted at first request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaUnit {
    pub unit_key: String,
    pub kind: MediaUnitKind,
    pub tmdb_id: u64,
    pub season: u32,
    pub episode: u32,
    pub title: String,
    pub year: Option<u32>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl MediaUnit {
    pub fn reference(&self) -> MediaUnitRef {
        MediaUnitRef {
            kind: self.kind,
            tmdb_id: self.tmdb_id,
            season: self.season,
            episode: self.episode,
        }
    }
}

/// Whether a location's bytes are usable, being checked, or gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationState {
    Pending,
    Present,
    Verifying,
    Missing,
    Failed,
}

impl LocationState {
    pub fn as_str(self) -> &'static str {
        match self {
            LocationState::Pending => "pending",
            LocationState::Present => "present",
            LocationState::Verifying => "verifying",
            LocationState::Missing => "missing",
            LocationState::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(LocationState::Pending),
            "present" => Some(LocationState::Present),
            "verifying" => Some(LocationState::Verifying),
            "missing" => Some(LocationState::Missing),
            "failed" => Some(LocationState::Failed),
            _ => None,
        }
    }
}

/// One place a unit's bytes live: a local file or a cloud replica.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaLocation {
    pub id: String,
    pub unit_key: String,
    pub provider: String,
    pub account_id: String,
    pub path: String,
    pub remote_id: String,
    pub origin: String,
    pub state: LocationState,
    pub is_primary: bool,
    pub size: Option<i64>,
    pub fingerprint: Option<String>,
    pub quality: Option<Value>,
    pub verified_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The single live attempt a unit may have. Historical attempts stay in the
/// table with a terminal status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    Queued,
    Searching,
    CandidateReady,
    WaitingConfirm,
    Transferring,
    Downloading,
    Importing,
    Verifying,
    Done,
    Failed,
    Cancelled,
    NeedsReview,
}

impl AttemptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptStatus::Queued => "queued",
            AttemptStatus::Searching => "searching",
            AttemptStatus::CandidateReady => "candidate_ready",
            AttemptStatus::WaitingConfirm => "waiting_confirm",
            AttemptStatus::Transferring => "transferring",
            AttemptStatus::Downloading => "downloading",
            AttemptStatus::Importing => "importing",
            AttemptStatus::Verifying => "verifying",
            AttemptStatus::Done => "done",
            AttemptStatus::Failed => "failed",
            AttemptStatus::Cancelled => "cancelled",
            AttemptStatus::NeedsReview => "needs_review",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => AttemptStatus::Queued,
            "searching" => AttemptStatus::Searching,
            "candidate_ready" => AttemptStatus::CandidateReady,
            "waiting_confirm" => AttemptStatus::WaitingConfirm,
            "transferring" => AttemptStatus::Transferring,
            "downloading" => AttemptStatus::Downloading,
            "importing" => AttemptStatus::Importing,
            "verifying" => AttemptStatus::Verifying,
            "done" => AttemptStatus::Done,
            "failed" => AttemptStatus::Failed,
            "cancelled" => AttemptStatus::Cancelled,
            "needs_review" => AttemptStatus::NeedsReview,
            _ => return None,
        })
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            AttemptStatus::Queued
                | AttemptStatus::Searching
                | AttemptStatus::CandidateReady
                | AttemptStatus::WaitingConfirm
                | AttemptStatus::Transferring
                | AttemptStatus::Downloading
                | AttemptStatus::Importing
                | AttemptStatus::Verifying
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionAttempt {
    pub id: String,
    pub unit_key: String,
    pub request_id: Option<String>,
    pub provider: String,
    pub status: AttemptStatus,
    pub error_code: String,
    pub error_message: String,
    pub candidate: Option<Value>,
    pub quote: Option<Value>,
    pub job_id: String,
    pub cost_points: u32,
    pub unlock_key: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
}

/// Where the bytes must live when acquisition finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    CloudOnly,
    LocalOnly,
    Both,
}

/// Which provider goes first. Independent of placement: a cloud-only library
/// can still use BT as a fallback into a local staging area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionOrder {
    CloudFirst,
    LocalFirst,
}

/// The operator-facing storage mode for one library. Cloud-only staging uses
/// BT only as a fallback and deletes the local staging file after verification;
/// hybrid modes retain both replicas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionMode {
    #[default]
    CloudOnlyStaged,
    HybridLocalFirst,
    LocalOnly,
}

impl AcquisitionMode {
    pub fn policy(self) -> AcquisitionPolicy {
        match self {
            AcquisitionMode::CloudOnlyStaged => AcquisitionPolicy::cloud_only_staged(),
            AcquisitionMode::HybridLocalFirst => AcquisitionPolicy::hybrid_local_first(),
            AcquisitionMode::LocalOnly => AcquisitionPolicy::local_only(),
        }
    }
}

/// The full acquisition policy for one library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionPolicy {
    pub placement: Placement,
    pub order: AcquisitionOrder,
    /// Download into a temporary local area when a cloud-only library has to
    /// fall back to BT/PT. Only meaningful with a non-cloud provider.
    pub local_staging: bool,
    /// After the cloud replica is verified, remove the staging file.
    pub evict_after_cloud_verified: bool,
}

impl AcquisitionPolicy {
    /// The mode for a library whose bytes live only in 115: try 115 first, and
    /// fall back to BT/PT through a local staging file.
    pub fn cloud_only_staged() -> Self {
        Self {
            placement: Placement::CloudOnly,
            order: AcquisitionOrder::CloudFirst,
            local_staging: true,
            evict_after_cloud_verified: true,
        }
    }

    /// The mode for a library that keeps both: download locally first, then
    /// upload a cloud replica.
    pub fn hybrid_local_first() -> Self {
        Self {
            placement: Placement::Both,
            order: AcquisitionOrder::LocalFirst,
            local_staging: false,
            evict_after_cloud_verified: false,
        }
    }

    /// A pure local library: no cloud replication, no cloud provider.
    pub fn local_only() -> Self {
        Self {
            placement: Placement::LocalOnly,
            order: AcquisitionOrder::LocalFirst,
            local_staging: false,
            evict_after_cloud_verified: false,
        }
    }

    pub fn requires_cloud(self) -> bool {
        matches!(self.placement, Placement::CloudOnly | Placement::Both)
    }

    pub fn requires_local(self) -> bool {
        matches!(self.placement, Placement::LocalOnly | Placement::Both)
    }

    /// The provider order to try when acquisition is needed. A cloud-only
    /// library without staging has no BT fallback; a staged one does.
    pub fn provider_order(self) -> &'static [&'static str] {
        match (self.placement, self.order) {
            (Placement::CloudOnly, _) if !self.local_staging => &[PROVIDER_CLOUD115],
            (Placement::CloudOnly, _) => &[PROVIDER_CLOUD115, PROVIDER_BT],
            (Placement::LocalOnly, _) => &[PROVIDER_BT],
            (Placement::Both, AcquisitionOrder::CloudFirst) => &[PROVIDER_CLOUD115, PROVIDER_BT],
            (Placement::Both, AcquisitionOrder::LocalFirst) => &[PROVIDER_BT, PROVIDER_CLOUD115],
        }
    }
}

/// What the library already holds for a unit, independent of provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedState {
    pub local: bool,
    pub cloud: bool,
}

impl OwnedState {
    pub fn both(&self) -> bool {
        self.local && self.cloud
    }

    pub fn any(&self) -> bool {
        self.local || self.cloud
    }
}

/// Provider readiness: why an acquisition cannot start yet.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderReadiness {
    pub ready: bool,
    #[serde(default)]
    pub blockers: Vec<String>,
}

impl ProviderReadiness {
    pub fn ready() -> Self {
        Self {
            ready: true,
            blockers: Vec::new(),
        }
    }

    pub fn blocked(reasons: impl IntoIterator<Item = String>) -> Self {
        let blockers: Vec<String> = reasons.into_iter().collect();
        Self {
            ready: blockers.is_empty(),
            blockers,
        }
    }
}

/// The one candidate shape every provider answers with. Provider-specific
/// fields ride in `extra`; this keeps one list usable by BT and 115.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub provider: String,
    pub title: String,
    pub size_bytes: Option<i64>,
    pub quality: Option<Value>,
    #[serde(default)]
    pub languages: Vec<String>,
    pub seeders: Option<u32>,
    pub price_points: Option<u32>,
    pub price_known: bool,
    pub grabbable: bool,
    pub reject_reason: Option<String>,
    #[serde(default)]
    pub extra: Value,
}

/// A provider's price answer before an irreversible unlock.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub points: Option<u32>,
    pub price_known: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[cfg(test)]
mod mode_tests {
    use super::{AcquisitionMode, AcquisitionOrder, Placement};

    #[test]
    fn library_acquisition_modes_resolve_to_the_expected_placement_and_order() {
        let cloud = AcquisitionMode::CloudOnlyStaged.policy();
        assert_eq!(cloud.placement, Placement::CloudOnly);
        assert_eq!(cloud.provider_order(), &["cloud115", "bt"]);
        assert!(cloud.local_staging && cloud.evict_after_cloud_verified);

        let local_first = AcquisitionMode::HybridLocalFirst.policy();
        assert_eq!(local_first.placement, Placement::Both);
        assert_eq!(local_first.order, AcquisitionOrder::LocalFirst);
        assert_eq!(local_first.provider_order(), &["bt", "cloud115"]);
        assert!(!local_first.evict_after_cloud_verified);

        let local = AcquisitionMode::LocalOnly.policy();
        assert_eq!(local.placement, Placement::LocalOnly);
        assert_eq!(local.provider_order(), &["bt"]);
    }
}
