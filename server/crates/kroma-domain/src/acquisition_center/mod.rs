//! The acquisition centre: which storage mode this deployment runs, what a
//! get/watch/subscription is aiming at, and the job shape the UI renders.
//!
//! Only `CloudIntake` is implemented. `LocalLibrary` and `LocalIntake` exist so
//! the mode switch, the API and the tables do not have to be redesigned when
//! the local and hybrid modes are built.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What the deployment stores media in. The setup wizard picks one; it does not
/// change where already-imported files live.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    #[default]
    CloudOnly,
    LocalOnly,
    Hybrid,
}

impl DeploymentMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DeploymentMode::CloudOnly => "cloud_only",
            DeploymentMode::LocalOnly => "local_only",
            DeploymentMode::Hybrid => "hybrid",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cloud_only" => Some(DeploymentMode::CloudOnly),
            "local_only" => Some(DeploymentMode::LocalOnly),
            "hybrid" => Some(DeploymentMode::Hybrid),
            _ => None,
        }
    }

    /// The modes the build can actually run today.
    pub fn available(self) -> bool {
        matches!(self, DeploymentMode::CloudOnly)
    }
}

/// Where a get's bytes should end up. `CloudIntake` is the 115 folder Symedia
/// watches; the other two are reserved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    #[default]
    CloudIntake,
    LocalLibrary,
    LocalIntake,
}

impl Destination {
    pub fn as_str(self) -> &'static str {
        match self {
            Destination::CloudIntake => "cloud_intake",
            Destination::LocalLibrary => "local_library",
            Destination::LocalIntake => "local_intake",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cloud_intake" => Some(Destination::CloudIntake),
            "local_library" => Some(Destination::LocalLibrary),
            "local_intake" => Some(Destination::LocalIntake),
            _ => None,
        }
    }

    pub fn available(self) -> bool {
        matches!(self, Destination::CloudIntake)
    }
}

/// A per-season follow point: full seasons the operator does not list are not
/// followed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonStart {
    pub season: u32,
    pub start_episode: u32,
}

/// One title followed by TMDB identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchTitle {
    pub id: String,
    pub tmdb_id: u64,
    pub kind: String,
    pub title: String,
    pub year: Option<u32>,
    pub poster_url: Option<String>,
    pub seasons: Vec<SeasonStart>,
    pub source_order: Vec<String>,
    pub enabled: bool,
    pub last_checked: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One 115 share link watched for new files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareSubscription {
    pub id: String,
    pub tmdb_id: u64,
    pub kind: String,
    pub title: String,
    pub poster_url: Option<String>,
    pub share_code: String,
    pub receive_code: String,
    pub seasons: Vec<SeasonStart>,
    pub enabled: bool,
    pub status: String,
    pub last_checked: Option<i64>,
    pub last_error: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One file a share subscription has listed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareFile {
    pub id: String,
    pub subscription_id: String,
    pub file_id: String,
    pub name: String,
    pub size: Option<i64>,
    pub sha: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub state: String,
    pub remote_id: Option<String>,
    pub error: String,
    pub first_seen: i64,
    pub updated_at: i64,
}

/// One get, watch attempt, or share transfer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionJob {
    pub id: String,
    pub kind: String,
    pub unit_key: String,
    pub tmdb_id: u64,
    pub title: String,
    pub media_kind: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub destination: Destination,
    pub source: String,
    pub candidate: Option<Value>,
    pub status: String,
    pub stage_detail: String,
    pub price_points: Option<u32>,
    pub price_known: bool,
    pub paid_confirmed: bool,
    pub remote_id: String,
    pub progress: Option<f64>,
    pub error_code: String,
    pub error_message: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
}

impl AcquisitionJob {
    /// A paid candidate has been quoted but not confirmed yet; nothing may be
    /// charged until this is true.
    pub fn waits_for_payment(&self) -> bool {
        self.status == "waiting_confirm"
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.status.as_str(), "organized" | "failed" | "cancelled")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_cloud_destination_runs_today() {
        assert!(Destination::CloudIntake.available());
        assert!(!Destination::LocalLibrary.available());
        assert!(!Destination::LocalIntake.available());
        assert!(DeploymentMode::CloudOnly.available());
        assert!(!DeploymentMode::Hybrid.available());
    }

    #[test]
    fn a_paid_quote_is_not_a_permission_to_charge() {
        let job = AcquisitionJob {
            id: "j".into(),
            kind: "one_shot".into(),
            unit_key: String::new(),
            tmdb_id: 1,
            title: "T".into(),
            media_kind: "movie".into(),
            season: None,
            episode: None,
            destination: Destination::CloudIntake,
            source: "madreel".into(),
            candidate: None,
            status: "waiting_confirm".into(),
            stage_detail: String::new(),
            price_points: Some(5),
            price_known: true,
            paid_confirmed: false,
            remote_id: String::new(),
            progress: None,
            error_code: String::new(),
            error_message: String::new(),
            created_at: 1,
            updated_at: 1,
            finished_at: None,
        };

        assert!(job.waits_for_payment());
        assert!(!job.paid_confirmed);
        assert!(!job.is_terminal());
    }
}
