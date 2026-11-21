//! The cloud-only staged tail of acquisition: when BT has imported a movie to
//! the library, upload it to 115, verify the STRM replacement, and evict only
//! after that replacement is in place. The 115 module owns the transfer code.

use anyhow::Result;
use serde::Deserialize;
use serde_json::json;

use crate::point::Point;
use kroma_domain::{
    AcquisitionAttempt, AttemptStatus, LocationState, MediaLocation, MediaUnitKind, PROVIDER_BT,
    PROVIDER_CLOUD115,
};
use kroma_requests::acquisition as planner;

use crate::db;
use crate::model::RequestStatus;
use crate::services::jobs;

use super::prelude::*;

/// A queued upload with no provider job id was interrupted between writing the
/// attempt and receiving the provider's job handle. Reopen it after a few ticks;
/// the module's instant-upload check makes a duplicate start safe.
const ORPHANED_UPLOAD_MS: i64 = 5 * 60 * 1000;

pub(super) const SPEC: Builtin = Builtin {
    key: JobKey("acquisition.staging"),
    category: Category::Acquisition,
    schedule: Some("*/2 * * * *"),
    triggers: &[],
    run,
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadStarted {
    job_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadStatus {
    state: String,
    pickcode: String,
    remote: String,
    size_bytes: u64,
    local_deleted: bool,
    error: String,
    warning: String,
}

pub(super) fn run(ctx: &JobContext) -> Result<()> {
    let point = Point::new("cloud115", ctx.state.contributions.clone());
    if !point.live() {
        return Ok(());
    }
    let mut polled = poll_cloud_uploads(ctx, &point)?;
    polled += start_cloud_uploads_for_completed_bt(ctx, &point)?;
    if polled > 0 {
        ctx.info(format!("advanced {polled} cloud-staging attempt(s)"));
    }
    Ok(())
}

fn poll_cloud_uploads(ctx: &JobContext, point: &Point) -> Result<usize> {
    let attempts = {
        let conn = ctx.state.db.get()?;
        db::attempts_by_provider(&conn, "cloud115_upload")?
    };
    let mut changed = 0;
    for attempt in attempts
        .into_iter()
        .filter(|attempt| attempt.status.is_active())
    {
        if ctx.cancelled() {
            break;
        }
        if attempt.job_id.is_empty() {
            let now = jobs::now_ms();
            if upload_attempt_is_orphaned(attempt.updated_at, now) {
                mark_upload_for_retry(
                    ctx,
                    &attempt,
                    "cloud115_job_id_missing",
                    "115 上传任务号未能保存；本地文件保留，将重新尝试上传",
                    now,
                )?;
                changed += 1;
            }
            continue;
        }
        let Some(answer) = point.call::<_, std::result::Result<UploadStatus, String>>(
            "upload-status",
            &json!({ "jobId": attempt.job_id }),
        ) else {
            continue;
        };
        let reply = match answer {
            Ok(reply) => reply,
            Err(error) if upload_job_was_lost(&error) => {
                let now = jobs::now_ms();
                mark_upload_for_retry(
                    ctx,
                    &attempt,
                    "cloud115_job_lost",
                    "115 模块重启后找不到原上传任务；本地文件保留，将重新尝试上传",
                    now,
                )?;
                changed += 1;
                continue;
            }
            Err(_) => continue,
        };
        match reply.state.as_str() {
            "queued" | "running" | "verifying" => {
                let status = match reply.state.as_str() {
                    "verifying" => AttemptStatus::Verifying,
                    _ => AttemptStatus::Transferring,
                };
                if status != attempt.status {
                    db::set_attempt_status(
                        &ctx.state.db,
                        &attempt.id,
                        status,
                        "",
                        "",
                        None,
                        jobs::now_ms(),
                    )?;
                    changed += 1;
                }
            }
            "done" => {
                let unit_exists = {
                    let conn = ctx.state.db.get()?;
                    db::get_media_unit(&conn, &attempt.unit_key)?.is_some()
                };
                if !unit_exists {
                    db::set_attempt_status(
                        &ctx.state.db,
                        &attempt.id,
                        AttemptStatus::NeedsReview,
                        "missing_unit",
                        "获取单元已不存在；已保留云端文件，未删除本地数据",
                        Some(jobs::now_ms()),
                        jobs::now_ms(),
                    )?;
                    changed += 1;
                    continue;
                }
                if reply.pickcode.trim().is_empty() {
                    db::set_attempt_status(
                        &ctx.state.db,
                        &attempt.id,
                        AttemptStatus::NeedsReview,
                        "missing_pickcode",
                        "115 已校验文件但未返回 pickcode；请检查云端条目",
                        Some(jobs::now_ms()),
                        jobs::now_ms(),
                    )?;
                    changed += 1;
                    continue;
                }
                let location_id = kroma_primitives::short_hash(&format!(
                    "cloud115|{}|{}",
                    attempt.unit_key, reply.pickcode
                ));
                let now = jobs::now_ms();
                let location = MediaLocation {
                    id: location_id,
                    unit_key: attempt.unit_key.clone(),
                    provider: PROVIDER_CLOUD115.into(),
                    account_id: String::new(),
                    path: reply.remote.clone(),
                    remote_id: reply.pickcode.clone(),
                    origin: "uploaded".into(),
                    state: LocationState::Present,
                    is_primary: true,
                    size: Some(reply.size_bytes.min(i64::MAX as u64) as i64),
                    fingerprint: None,
                    quality: None,
                    verified_at: Some(now),
                    created_at: now,
                    updated_at: now,
                };
                let conn = ctx.state.db.get()?;
                db::upsert_media_location(&conn, &location)?;
                let evict_local = upload_should_evict(ctx, &attempt)?;
                let (status, code, message) = if upload_completion_satisfied(evict_local, &reply) {
                    (AttemptStatus::Done, "", "")
                } else if !reply.warning.is_empty() {
                    (
                        AttemptStatus::NeedsReview,
                        "local_staging_kept",
                        reply.warning.as_str(),
                    )
                } else if !evict_local && reply.local_deleted {
                    (
                        AttemptStatus::NeedsReview,
                        "local_replica_missing",
                        "混合存放策略要求保留本地副本，但上传模块报告本地文件已删除；请检查媒体库文件",
                    )
                } else {
                    (
                        AttemptStatus::NeedsReview,
                        "local_staging_kept",
                        "云端已验证，但本地暂存文件仍在；请检查后手动清理",
                    )
                };
                db::set_attempt_status(
                    &ctx.state.db,
                    &attempt.id,
                    status,
                    code,
                    message,
                    Some(now),
                    now,
                )?;
                if let Some(request_id) = attempt.request_id.as_deref() {
                    if let Some(request) = db::get_request(&conn, request_id)? {
                        ctx.state.events.publish(
                            crate::infra::events::ServerEvent::RequestUpdated {
                                id: request.id,
                                status: request.status.as_str().to_string(),
                            },
                        );
                    }
                }
                changed += 1;
            }
            "failed" => {
                db::set_attempt_status(
                    &ctx.state.db,
                    &attempt.id,
                    AttemptStatus::Failed,
                    "cloud115_upload_failed",
                    &reply.error,
                    Some(jobs::now_ms()),
                    jobs::now_ms(),
                )?;
                changed += 1;
            }
            _ => {}
        }
    }
    Ok(changed)
}

fn mark_upload_for_retry(
    ctx: &JobContext,
    attempt: &AcquisitionAttempt,
    code: &str,
    message: &str,
    now: i64,
) -> Result<()> {
    db::set_attempt_status(
        &ctx.state.db,
        &attempt.id,
        AttemptStatus::NeedsReview,
        code,
        message,
        Some(now),
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        library_for_path, upload_attempt_is_orphaned, upload_attempt_suppresses_retry,
        upload_completion_satisfied, upload_job_was_lost, UploadStatus, ORPHANED_UPLOAD_MS,
    };
    use crate::services::settings::LibraryDef;
    use kroma_domain::{AcquisitionAttempt, AttemptStatus};

    #[test]
    fn an_upload_without_a_saved_job_id_expires_only_after_the_retry_window() {
        assert!(!upload_attempt_is_orphaned(
            100,
            100 + ORPHANED_UPLOAD_MS - 1
        ));
        assert!(upload_attempt_is_orphaned(100, 100 + ORPHANED_UPLOAD_MS));
        assert!(
            !upload_attempt_is_orphaned(200, 100),
            "clock rollback is safe"
        );
    }

    #[test]
    fn a_provider_restart_message_marks_a_lost_upload_job() {
        assert!(upload_job_was_lost("任务不存在（模块可能重启过）"));
        assert!(!upload_job_was_lost("115 请求频率过高"));
    }

    #[test]
    fn cloud_staging_uses_the_deepest_video_library_root() {
        let libraries = vec![
            LibraryDef {
                id: "movies".into(),
                name: "Movies".into(),
                kind: "movies".into(),
                folders: vec!["/media".into()],
                auto_scan: true,
            },
            LibraryDef {
                id: "local-only".into(),
                name: "Local".into(),
                kind: "movies".into(),
                folders: vec!["/media/local".into()],
                auto_scan: true,
            },
            LibraryDef {
                id: "photos".into(),
                name: "Photos".into(),
                kind: "photos".into(),
                folders: vec!["/media/photos".into()],
                auto_scan: true,
            },
        ];
        assert_eq!(
            library_for_path(&libraries, "/media/local/Film/film.mkv").map(|lib| lib.id.as_str()),
            Some("local-only")
        );
        assert_eq!(
            library_for_path(&libraries, "/media/photos/family.jpg").map(|lib| lib.id.as_str()),
            None,
            "photo libraries must never enter the video staging pipeline"
        );
    }

    #[test]
    fn upload_completion_requires_the_local_file_state_to_match_policy() {
        let kept = UploadStatus {
            local_deleted: false,
            ..UploadStatus::default()
        };
        let deleted = UploadStatus {
            local_deleted: true,
            ..UploadStatus::default()
        };

        assert!(upload_completion_satisfied(false, &kept));
        assert!(!upload_completion_satisfied(false, &deleted));
        assert!(!upload_completion_satisfied(true, &kept));
        assert!(upload_completion_satisfied(true, &deleted));
    }

    #[test]
    fn upload_warnings_and_failed_cloud_only_eviction_require_review() {
        let kept = UploadStatus {
            local_deleted: false,
            ..UploadStatus::default()
        };
        assert!(!upload_completion_satisfied(true, &kept));

        let conflict = UploadStatus {
            warning: "STRM conflict".into(),
            ..UploadStatus::default()
        };
        assert!(!upload_completion_satisfied(false, &conflict));
    }

    #[test]
    fn a_needs_review_upload_is_not_retried_without_manual_resolution() {
        let attempt = AcquisitionAttempt {
            id: "attempt".into(),
            unit_key: "movie:1".into(),
            request_id: None,
            provider: "cloud115_upload".into(),
            status: AttemptStatus::NeedsReview,
            error_code: "local_staging_kept".into(),
            error_message: String::new(),
            candidate: None,
            quote: None,
            job_id: "job".into(),
            cost_points: 0,
            unlock_key: None,
            created_at: 1,
            updated_at: 1,
            finished_at: Some(1),
        };

        assert!(upload_attempt_suppresses_retry(&attempt, 1_000_000));
    }
}
fn upload_attempt_is_orphaned(updated_at: i64, now: i64) -> bool {
    now.saturating_sub(updated_at) >= ORPHANED_UPLOAD_MS
}

fn upload_job_was_lost(error: &str) -> bool {
    error.contains("任务不存在")
}

fn start_cloud_uploads_for_completed_bt(ctx: &JobContext, point: &Point) -> Result<usize> {
    let attempts = {
        let conn = ctx.state.db.get()?;
        db::attempts_by_provider(&conn, PROVIDER_BT)?
    };
    let mut started = 0;
    for bt in attempts {
        if ctx.cancelled() {
            break;
        }
        if bt.request_id.is_none()
            || !matches!(bt.status, AttemptStatus::Queued | AttemptStatus::Done)
        {
            continue;
        }
        let request = {
            let conn = ctx.state.db.get()?;
            db::get_request(&conn, bt.request_id.as_deref().unwrap_or_default())?
        };
        let Some(request) = request else { continue };
        if !matches!(request.status, RequestStatus::Available) {
            continue;
        }
        let unit = {
            let conn = ctx.state.db.get()?;
            db::get_media_unit(&conn, &bt.unit_key)?
        };
        let Some(unit) = unit else { continue };
        if unit.kind != MediaUnitKind::Movie {
            // The first cloud-staging job handles a movie unit. Show packs need
            // an episode-to-file coverage join before a safe transfer can start.
            continue;
        }
        let cloud_present = planner::owned(&ctx.state, &unit.reference())?.cloud;
        if cloud_present {
            if bt.status != AttemptStatus::Done {
                db::set_attempt_status(
                    &ctx.state.db,
                    &bt.id,
                    AttemptStatus::Done,
                    "",
                    "",
                    Some(jobs::now_ms()),
                    jobs::now_ms(),
                )?;
                started += 1;
            }
            continue;
        }
        if has_recent_upload_attempt(ctx, &bt.unit_key)? {
            continue;
        }
        let requested_library = bt
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.get("libraryId"))
            .and_then(serde_json::Value::as_str);
        let file = {
            let conn = ctx.state.db.get()?;
            db::movie_files_by_tmdb(&conn, unit.tmdb_id, requested_library)?
                .into_iter()
                .map(|file| file.path)
                .find(|path| is_staging_video(path))
        };
        let Some(path) = file else { continue };
        let Some(policy) = library_policy_for_path(ctx, &path)? else {
            // A file whose containing library cannot be resolved is left alone.
            continue;
        };
        if !policy.requires_cloud() {
            if bt.status != AttemptStatus::Done {
                db::set_attempt_status(
                    &ctx.state.db,
                    &bt.id,
                    AttemptStatus::Done,
                    "",
                    "",
                    Some(jobs::now_ms()),
                    jobs::now_ms(),
                )?;
                started += 1;
            }
            continue;
        }
        // Close the BT attempt before opening the placement attempt. The unique
        // active-attempt index allows one live row per unit.
        if bt.status != AttemptStatus::Done {
            db::set_attempt_status(
                &ctx.state.db,
                &bt.id,
                AttemptStatus::Done,
                "",
                "",
                Some(jobs::now_ms()),
                jobs::now_ms(),
            )?;
        }
        let now = jobs::now_ms();
        let upload_id =
            kroma_primitives::short_hash(&format!("cloud115-upload|{}|{now}", bt.unit_key));
        let upload = AcquisitionAttempt {
            id: upload_id.clone(),
            unit_key: bt.unit_key.clone(),
            request_id: bt.request_id.clone(),
            provider: "cloud115_upload".into(),
            status: AttemptStatus::Queued,
            error_code: String::new(),
            error_message: String::new(),
            candidate: Some(json!({
                "path": path,
                "deleteLocal": policy.evict_after_cloud_verified,
            })),
            quote: None,
            job_id: String::new(),
            cost_points: 0,
            unlock_key: None,
            created_at: now,
            updated_at: now,
            finished_at: None,
        };
        db::create_attempt(&ctx.state.db, &upload)?;
        let answer = point.call::<_, std::result::Result<UploadStarted, String>>(
            "upload",
            &json!({ "path": path, "deleteLocal": policy.evict_after_cloud_verified }),
        );
        match answer {
            Some(Ok(reply)) if !reply.job_id.trim().is_empty() => {
                db::set_attempt_job(&ctx.state.db, &upload_id, &reply.job_id, jobs::now_ms())?;
                db::set_attempt_status(
                    &ctx.state.db,
                    &upload_id,
                    AttemptStatus::Transferring,
                    "",
                    "",
                    None,
                    jobs::now_ms(),
                )?;
            }
            Some(Ok(_)) => db::set_attempt_status(
                &ctx.state.db,
                &upload_id,
                AttemptStatus::Failed,
                "provider_no_job",
                "115 上传模块没有返回任务号；本地文件保留",
                Some(jobs::now_ms()),
                jobs::now_ms(),
            )?,
            Some(Err(error)) => db::set_attempt_status(
                &ctx.state.db,
                &upload_id,
                AttemptStatus::Failed,
                "provider_error",
                &error,
                Some(jobs::now_ms()),
                jobs::now_ms(),
            )?,
            None => db::set_attempt_status(
                &ctx.state.db,
                &upload_id,
                AttemptStatus::Failed,
                "provider_unavailable",
                "115 上传模块暂时不可用；本地文件保留",
                Some(jobs::now_ms()),
                jobs::now_ms(),
            )?,
        }
        started += 1;
    }
    Ok(started)
}

fn has_recent_upload_attempt(ctx: &JobContext, unit_key: &str) -> Result<bool> {
    let attempts = {
        let conn = ctx.state.db.get()?;
        db::attempts_for_unit(&conn, unit_key)?
    };
    let now = jobs::now_ms();
    Ok(attempts.into_iter().any(|attempt| {
        attempt.provider == "cloud115_upload" && upload_attempt_suppresses_retry(&attempt, now)
    }))
}

fn upload_attempt_suppresses_retry(attempt: &AcquisitionAttempt, now: i64) -> bool {
    attempt.status.is_active()
        || attempt.status == AttemptStatus::NeedsReview
        || (attempt.status == AttemptStatus::Failed
            && now.saturating_sub(attempt.updated_at) < 15 * 60 * 1000)
        || (attempt.status == AttemptStatus::Done && attempt.error_code != "local_staging_kept")
}

fn upload_completion_satisfied(evict_local: bool, reply: &UploadStatus) -> bool {
    reply.warning.is_empty() && reply.local_deleted == evict_local
}

fn upload_should_evict(ctx: &JobContext, attempt: &AcquisitionAttempt) -> Result<bool> {
    if let Some(should_evict) = attempt
        .candidate
        .as_ref()
        .and_then(|candidate| candidate.get("deleteLocal"))
        .and_then(serde_json::Value::as_bool)
    {
        return Ok(should_evict);
    }
    // Older in-flight attempts predate the stored placement decision. Resolve
    // their policy from the library, defaulting to eviction-safe behavior if
    // the target cannot be identified.
    if let Some(path) = attempt
        .candidate
        .as_ref()
        .and_then(|candidate| candidate.get("path"))
        .and_then(serde_json::Value::as_str)
    {
        if let Some(policy) = library_policy_for_path(ctx, path)? {
            return Ok(policy.evict_after_cloud_verified);
        }
    }
    Ok(true)
}

fn library_policy_for_path(
    ctx: &JobContext,
    path: &str,
) -> Result<Option<kroma_domain::AcquisitionPolicy>> {
    let libraries =
        crate::services::settings::library_defs(&ctx.state.settings, &ctx.state.config)?;
    Ok(library_for_path(&libraries, path).map(|library| {
        crate::services::settings::library_acquisition(&ctx.state.settings, &library.id).policy()
    }))
}

fn library_for_path<'a>(
    libraries: &'a [crate::services::settings::LibraryDef],
    path: &str,
) -> Option<&'a crate::services::settings::LibraryDef> {
    let path = std::path::Path::new(path);
    libraries
        .iter()
        .flat_map(|library| {
            library.folders.iter().filter_map(move |folder| {
                let root = std::path::Path::new(folder);
                path.starts_with(root)
                    .then_some((root.components().count(), library))
            })
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, library)| library)
        .filter(|library| {
            library.kind.is_empty() || matches!(library.kind.as_str(), "movies" | "mixed")
        })
}

fn is_staging_video(path: &str) -> bool {
    let path = std::path::Path::new(path);
    if !path.is_file() || path.extension().is_none() {
        return false;
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "mkv" | "mp4" | "avi" | "mov" | "wmv" | "webm" | "m4v" | "mpeg" | "mpg" | "ts" | "m2ts"
    )
}
