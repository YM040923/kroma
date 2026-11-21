//! `acquire` and `status`: the price gate in front of the source's unlock, and a
//! proxy to the source's own job record.
//!
//! Unlocking a share spends points and cannot be undone, so nothing starts
//! without an explicit confirmation whenever the price is unknown or positive.
//! A zero price needs none: the source already owns that share, so there is
//! nothing to spend.

use axum::Json;
use kroma_module_sdk::host::port_reply;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::search::source_kind;
use super::{readiness_of, source_json};
use crate::settings::Config;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquireBody {
    /// The core's idempotency key; a replay returns the existing source job.
    #[serde(default)]
    pub attempt_id: String,
    #[serde(default)]
    pub unit_key: String,
    /// `movie` or `show`.
    pub kind: String,
    pub tmdb_id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub library: Option<String>,
    /// Exact candidate returned by `/search`; source-specific data remains in
    /// `extra`, and the core echoes this object unchanged for confirmation.
    #[serde(default)]
    pub candidate: Option<Value>,
    /// Flat fields remain accepted for existing module-local callers/tests.
    #[serde(default)]
    pub resource: String,
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub price: Option<i64>,
    #[serde(default)]
    pub link: String,
    /// Only meaningful for a show.
    #[serde(default)]
    pub season: i64,
    /// The caller has seen the price and means it.
    #[serde(default)]
    pub confirmed: bool,
}

/// What an irreversible unlock would cost, for the caller to show and confirm.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub points: Option<u32>,
    pub price_known: bool,
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquireReply {
    /// `started`, `needs_confirm` or `failed`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<Quote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What the gate decided about one requested unlock.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Start it: free, or already confirmed.
    Start,
    /// Ask first.
    Confirm(Quote),
}

/// The one rule this route exists for. An unknown price is not zero: the source
/// only reports a real price after probing, and unlocking without it is exactly
/// how points get spent by accident.
pub(crate) fn gate(price: Option<i64>, confirmed: bool) -> Gate {
    let known = price.and_then(|price| u32::try_from(price).ok());
    match known {
        None if confirmed => Gate::Start,
        None => Gate::Confirm(Quote {
            points: None,
            price_known: false,
            note: "来源没有报出这次解锁的价格；确认后仍可能扣点".into(),
        }),
        Some(points) if points > 0 && !confirmed => Gate::Confirm(Quote {
            points: Some(points),
            price_known: true,
            note: format!("解锁这个资源将扣 {points} 积分"),
        }),
        Some(_) => Gate::Start,
    }
}

fn failed(error: impl Into<String>) -> AcquireReply {
    AcquireReply {
        status: "failed".into(),
        error: Some(error.into()),
        ..AcquireReply::default()
    }
}

pub(super) async fn acquire(Json(body): Json<AcquireBody>) -> Json<Result<AcquireReply, String>> {
    port_reply(move || {
        let config = Config::load();
        Ok(acquire_with(&config, &body))
    })
    .await
}

#[derive(Debug, Clone)]
struct CandidateFields {
    resource: String,
    index: i64,
    price: Option<i64>,
    link: String,
    already_unlocked: bool,
}

fn candidate_fields(body: &AcquireBody) -> CandidateFields {
    let candidate = body.candidate.as_ref();
    let extra = candidate.and_then(|value| value.get("extra"));
    let text = |key: &str| {
        extra
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .or_else(|| candidate.and_then(|value| value.get(key)).and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let integer = |key: &str| {
        extra
            .and_then(|value| value.get(key))
            .and_then(as_i64)
            .or_else(|| candidate.and_then(|value| value.get(key)).and_then(as_i64))
    };
    let already_unlocked = extra
        .and_then(|value| value.get("alreadyUnlocked"))
        .or_else(|| candidate.and_then(|value| value.get("alreadyUnlocked")))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let resource = {
        let resource = text("resource");
        if resource.is_empty() {
            text("fileName")
        } else {
            resource
        }
    };
    let link = text("link");
    let index = integer("index").unwrap_or(body.index).max(0);
    let price = if already_unlocked {
        Some(0)
    } else {
        integer("price")
            .or_else(|| integer("pricePoints"))
            .or(body.price)
    };
    CandidateFields {
        resource: if resource.is_empty() { body.resource.trim().into() } else { resource },
        index,
        price,
        link: if link.is_empty() { body.link.trim().into() } else { link },
        already_unlocked,
    }
}

fn as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
}

fn acquire_with(config: &Config, body: &AcquireBody) -> AcquireReply {
    let fields = candidate_fields(body);
    if !body.attempt_id.trim().is_empty() {
        let existing = crate::store::load()
            .acquisitions
            .into_iter()
            .rev()
            .find(|entry| entry.attempt_id == body.attempt_id.trim());
        if let Some(entry) = existing {
            if entry.index != fields.index || entry.resource != fields.resource {
                return failed("attemptId 已绑定到另一个资源，拒绝重复解锁");
            }
            if !entry.job_id.is_empty() {
                return AcquireReply {
                    status: "started".into(),
                    job_id: Some(entry.job_id),
                    ..AcquireReply::default()
                };
            }
            if entry.state == "submitting" {
                return failed("上次解锁结果未确认；请先核查任务，避免重复扣点");
            }
        }
    }
    let readiness = readiness_of(config);
    if !readiness.ready {
        return failed(readiness.blockers.join("；"));
    }
    let Some(kind) = source_kind(&body.kind) else {
        return failed(format!(
            "未知类型：{}（只支持 movie / show）",
            body.kind.trim()
        ));
    };
    if body.tmdb_id <= 0 {
        return failed("缺少 TMDB 标识");
    }
    match gate(fields.price, body.confirmed || fields.already_unlocked) {
        Gate::Confirm(quote) => AcquireReply {
            status: "needs_confirm".into(),
            quote: Some(quote),
            ..AcquireReply::default()
        },
        Gate::Start => start(config, body, &fields, kind),
    }
}

/// The existing unlock/transfer flow, as the source exposes it: `202` with a job
/// id, polled through [`status`]. An attempt id is recorded before the write so a
/// lost response cannot make a retry spend points twice.
fn start(config: &Config, body: &AcquireBody, fields: &CandidateFields, kind: &str) -> AcquireReply {
    record(body, fields, "", "submitting", "");
    let mut payload = serde_json::json!({
        "tmdb": body.tmdb_id,
        "type": kind,
        "index": fields.index,
        "transfer": true,
    });
    if body.season > 0 {
        payload["season"] = serde_json::json!(body.season);
    }
    let value = match source_json(config, "/unlock", Some(&payload)) {
        Ok(value) => value,
        Err(error) => {
            record(body, fields, "", "failed", &error);
            return failed(error);
        }
    };
    let job_id = value
        .get("jobId")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|job_id| !job_id.is_empty())
        .map(str::to_string);
    let Some(job_id) = job_id else {
        let error = "订阅源没有给出任务号";
        record(body, fields, "", "failed", error);
        return failed(error);
    };
    record(body, fields, &job_id, "started", "");
    AcquireReply {
        status: "started".into(),
        job_id: Some(job_id),
        ..AcquireReply::default()
    }
}

fn record(body: &AcquireBody, fields: &CandidateFields, job_id: &str, state: &str, error: &str) {
    let entry = crate::store::AcquireEntry {
        at: crate::store::now_ms(),
        attempt_id: body.attempt_id.trim().to_string(),
        kind: body.kind.trim().to_string(),
        tmdb_id: body.tmdb_id,
        resource: fields.resource.clone(),
        index: fields.index,
        price: fields.price,
        link: fields.link.clone(),
        job_id: job_id.to_string(),
        state: state.to_string(),
        error: error.to_string(),
    };
    let _ = crate::store::patch(|stored| {
        if !entry.attempt_id.is_empty() {
            if let Some(existing) = stored
                .acquisitions
                .iter_mut()
                .find(|existing| existing.attempt_id == entry.attempt_id)
            {
                *existing = entry.clone();
                return;
            }
        }
        stored.acquisitions.push(entry);
        let overflow = stored.acquisitions.len().saturating_sub(50);
        if overflow > 0 {
            stored.acquisitions.drain(0..overflow);
        }
    });
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusBody {
    #[serde(default)]
    pub job_id: String,
}

pub(super) async fn status(Json(body): Json<StatusBody>) -> Json<Result<Value, String>> {
    port_reply(move || {
        let config = Config::load();
        Ok(status_with(&config, &body.job_id))
    })
    .await
}

/// The source's own job record, verbatim, so the caller keeps `result.unlocked`
/// (the link and the price actually paid). A failure here is reported in the
/// normalised shape instead of an `Err`, because a poll that cannot reach the
/// source is not a protocol error.
fn status_with(config: &Config, job_id: &str) -> Value {
    let job_id = job_id.trim();
    if job_id.is_empty() {
        return normalized("failed", "", "缺少任务号");
    }
    if !safe_id(job_id) {
        return normalized("failed", "", "任务号无效");
    }
    match source_json(config, &format!("/job/{job_id}"), None) {
        Ok(value) if value.is_object() => {
            annotate(job_id, &value);
            value
        }
        Ok(_) => normalized("unknown", "", "订阅源返回的不是任务对象"),
        Err(error) => normalized("unknown", "", &error),
    }
}

fn normalized(state: &str, step: &str, error: &str) -> Value {
    serde_json::json!({ "state": state, "step": step, "error": error })
}

/// A job id travels into the source's URL path, so it may only be the id
/// characters a UUID is made of.
fn safe_id(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id.len() <= 80
        && job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Fold the source's status back into the log entry, when it changed.
fn annotate(job_id: &str, value: &Value) {
    let Some(state) = value.get("status").and_then(Value::as_str) else {
        return;
    };
    let changed = crate::store::load()
        .acquisitions
        .iter()
        .rev()
        .find(|entry| entry.job_id == job_id)
        .is_some_and(|entry| entry.state != state);
    if !changed {
        return;
    }
    let _ = crate::store::patch(|stored| {
        if let Some(entry) = stored
            .acquisitions
            .iter_mut()
            .rev()
            .find(|entry| entry.job_id == job_id)
        {
            entry.state = state.to_string();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote_of(gate: Gate) -> Quote {
        match gate {
            Gate::Confirm(quote) => quote,
            Gate::Start => panic!("the gate started an unlock the test expected to be held"),
        }
    }

    #[test]
    fn an_unknown_price_always_needs_a_confirmation() {
        let quote = quote_of(gate(None, false));
        assert!(!quote.price_known);
        assert_eq!(quote.points, None);
        assert!(!quote.note.is_empty());

        // The caller's explicit confirmation may approve an unknown price; it
        // must never be treated as free before that click.
        assert_eq!(gate(None, true), Gate::Start);
    }

    #[test]
    fn a_positive_price_is_held_until_it_is_confirmed() {
        let quote = quote_of(gate(Some(4), false));
        assert!(quote.price_known);
        assert_eq!(quote.points, Some(4));
        assert!(quote.note.contains('4'), "{}", quote.note);

        assert_eq!(gate(Some(4), true), Gate::Start);
    }

    #[test]
    fn a_free_unlock_starts_without_a_confirmation() {
        assert_eq!(gate(Some(0), false), Gate::Start, "already unlocked costs nothing");
        assert_eq!(gate(Some(0), true), Gate::Start);
    }

    #[test]
    fn a_negative_or_absurd_price_reads_as_unknown() {
        assert!(!quote_of(gate(Some(-1), false)).price_known);
        assert!(!quote_of(gate(Some(i64::from(u32::MAX) + 1), false)).price_known);
    }

    fn configured() -> Config {
        Config {
            cookie: "UID=1_A1_2; CID=x".into(),
            source_url: "http://127.0.0.1:9".into(),
            ..Config::default()
        }
    }

    #[test]
    fn an_acquire_for_a_configured_provider_but_a_bad_kind_answers_failed() {
        let config = configured();
        let reply = acquire_with(
            &config,
            &AcquireBody {
                kind: "sideways".into(),
                tmdb_id: 155,
                ..AcquireBody::default()
            },
        );
        assert_eq!(reply.status, "failed");
        assert!(reply.error.unwrap().contains("未知类型"));
    }

    #[test]
    fn an_acquire_without_a_cookie_answers_failed_with_the_blockers() {
        let reply = acquire_with(
            &Config::default(),
            &AcquireBody {
                kind: "movie".into(),
                tmdb_id: 155,
                price: Some(0),
                confirmed: true,
                ..AcquireBody::default()
            },
        );
        assert_eq!(reply.status, "failed");
        let error = reply.error.unwrap();
        assert!(error.contains("Cookie"), "{error}");
        assert!(error.contains("订阅源"), "{error}");
    }

    #[test]
    fn a_positive_price_never_reaches_the_source_unconfirmed() {
        let config = configured();
        let reply = acquire_with(
            &config,
            &AcquireBody {
                kind: "movie".into(),
                tmdb_id: 155,
                index: 2,
                price: Some(9),
                ..AcquireBody::default()
            },
        );
        // The gate answers before any transport is attempted: nothing about this
        // needs the source to be reachable.
        assert_eq!(reply.status, "needs_confirm");
        assert_eq!(reply.quote.unwrap().points, Some(9));
        assert!(reply.job_id.is_none());
    }

    #[test]
    fn a_status_poll_without_a_usable_job_id_answers_the_normalised_shape() {
        let config = Config::default();
        let missing = status_with(&config, "  ");
        assert_eq!(missing["state"], "failed");
        assert_eq!(missing["step"], "");

        let hostile = status_with(&config, "../../etc/passwd");
        assert_eq!(hostile["state"], "failed");
        assert!(hostile["error"].as_str().unwrap().contains("任务号无效"));
    }

    #[test]
    fn a_job_id_is_limited_to_the_characters_an_id_is_made_of() {
        assert!(safe_id("6f1c2a9e-3b1d-4c7e-9d2a-0b1c2d3e4f5a"));
        assert!(safe_id("job_1"));
        assert!(!safe_id(""));
        assert!(!safe_id("a/b"));
        assert!(!safe_id(&"a".repeat(81)));
    }

    #[test]
    fn the_replies_keep_the_points_field_names() {
        let value = serde_json::to_value(AcquireReply {
            status: "needs_confirm".into(),
            quote: Some(Quote {
                points: Some(4),
                price_known: true,
                note: "note".into(),
            }),
            ..AcquireReply::default()
        })
        .unwrap();
        assert_eq!(value["status"], "needs_confirm");
        assert_eq!(value["quote"]["points"], 4);
        assert_eq!(value["quote"]["priceKnown"], true);
        assert_eq!(value["quote"]["note"], "note");
        assert!(
            value.get("jobId").is_none() && value.get("error").is_none(),
            "absent sides stay out of the answer: {value}"
        );

        let started = serde_json::to_value(AcquireReply {
            status: "started".into(),
            job_id: Some("job-1".into()),
            ..AcquireReply::default()
        })
        .unwrap();
        assert_eq!(started["jobId"], "job-1");
    }

    #[test]
    fn a_status_poll_for_a_source_that_is_not_configured_says_so() {
        let value = status_with(&Config::default(), "job-1");
        assert_eq!(value["state"], "unknown");
        assert!(value["error"].as_str().unwrap().contains("订阅源"));
    }
}