//! `/api/acquisition`: the one entry point that turns "I want this title" into
//! either an owned answer, a 115 cloud acquisition (free or confirmed), or a
//! normal request for the existing BT path.
//!
//! # The `cloud115` point
//!
//! The core holds no drive logic. It resolves whichever module answers the
//! `cloud115` point and speaks opaque JSON with it. This file is the contract
//! the provider implements (`tv.kroma.cloud115`), and every call is wrapped so a
//! module that is absent, disabled or failing falls back to BT on `/start`
//! instead of 500ing:
//!
//! | method | request | reply |
//! |---|---|---|
//! | `readiness` | `{ "library": null \| "movies" }` | [`ProviderReadiness`] (`{ ready, blockers }`) |
//! | `search` | `{ kind, tmdbId, title, year, library, unitKey }` | a list of candidates, or `{ "candidates": [...] }` |
//! | `acquire` | `{ attemptId, unitKey, kind, tmdbId, title, year, library, candidate, confirmed }` | `{ "jobId": "..." }`, optionally `points`, `unlockKey` |
//! | `status` | `{ attemptId, jobId, unitKey }` | `{ "status": "queued"\|"transferring"\|"done"\|"failed"\|..., "errorCode"?, "errorMessage"? }` |
//!
//! A candidate is the shared [`Candidate`] shape every provider answers with:
//! `{ id, provider, title, sizeBytes, quality, languages, seeders, pricePoints,
//! priceKnown, grabbable, rejectReason, extra }`. `extra.score` (when present)
//! ranks the search, and `extra.unlocked` marks a resource the account already
//! holds, which needs no confirmation whatever it would cost.
//!
//! `acquire` must be idempotent per `attemptId`: the core never charges one
//! attempt twice on its own, and re-issues `acquire` for the SAME attempt id
//! when a crash lost the provider's reply (there is no job id to hand back).
//!
//! # What this slice does not do
//!
//! - Shows keep the existing request + BT ledger: they create/merge a normal
//!   request and stop there.
//! - Movie placement and provider order are resolved from the selected (or
//!   configured default) video library's acquisition mode; the legacy default is
//!   cloud-only with a local staging fallback.
//! - A BT fallback is recorded as one `queued` attempt pointing at its request;
//!   the download/import ledger stays the source of truth for that provider.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use kroma_domain::{
    AcquisitionAttempt, AcquisitionPolicy, AttemptStatus, Candidate, MediaLocation, MediaUnitRef,
    OwnedState, ProviderReadiness, PROVIDER_BT, PROVIDER_CLOUD115,
};
use kroma_module_host::Resolver;
use kroma_requests::acquisition as planner;
use kroma_requests::acquisition::NextAction;
use kroma_requests::lifecycle as requests;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::error::{json_error, lerr};
use crate::api::extract::AuthUser;
use crate::api::permission::require;
use crate::api::point;
use crate::api::util::{blocking, service};
use crate::db;
use crate::i18n;
use crate::model::{CreateRequestBody, EpisodeRef, Permission, RequestKind, RequestStatus, User};
use crate::services::{auth, jobs, scan};
use crate::state::SharedState;

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/acquisition/start", post(start))
        .route("/acquisition/candidates", post(candidates))
        .route("/acquisition/confirm", post(confirm))
        .route("/acquisition/status", get(status))
        .route("/acquisition/tasks", get(tasks))
}

/// Which kind of title the caller wants. Only `movie` gets the provider flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Movie,
    Show,
}

/// `POST /api/acquisition/start`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBody {
    kind: Kind,
    tmdb_id: u64,
    title: String,
    #[serde(default)]
    year: Option<u32>,
    /// For shows: the seasons to request (absent = the whole show).
    #[serde(default)]
    seasons: Option<Vec<u32>>,
    /// For shows: individual episodes, unioned with `seasons`.
    #[serde(default)]
    episodes: Option<Vec<EpisodeRef>>,
    /// The library the title belongs to. Carried to the provider today; the
    /// per-library placement policy lands with the library settings work.
    #[serde(default)]
    library: Option<String>,
    /// For a show: the season the panel was listing. Redundant with a
    /// one-element `seasons`, but the panel sends it plainly and the transfer
    /// needs it.
    #[serde(default)]
    season: Option<u32>,
    /// The release the caller picked in the acquisition panel, echoed back
    /// exactly as the search returned it. Absent means the server chooses, which
    /// is the plain `/start` behaviour.
    #[serde(default)]
    candidate: Option<Value>,
}

/// `POST /api/acquisition/confirm`: the identification fields plus the exact
/// candidate the search returned, echoed back.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmBody {
    kind: Kind,
    tmdb_id: u64,
    title: String,
    #[serde(default)]
    year: Option<u32>,
    #[serde(default)]
    library: Option<String>,
    #[serde(default)]
    season: Option<u32>,
    candidate: Value,
}

/// `GET /api/acquisition/status?attemptId=…`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusQuery {
    attempt_id: String,
}

/// `POST /api/acquisition/start` — start acquiring one title behind the
/// `requests.create` capability. A film walks the cloud-only staged policy
/// (115 first, BT fallback); a show is merged into a normal request.
pub async fn start(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Json(body): Json<StartBody>,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    if body.tmdb_id == 0 || body.title.trim().is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "a title needs a TMDB id and a name",
        ));
    }
    // Resolution reads the supervisor's registry and never the network, so it
    // is safe inline. A missing point is not an error here: the plan falls
    // through to the BT provider.
    let cloud = point::require(&state, &user, PROVIDER_CLOUD115).ok();
    // A candidate in the body is the panel's explicit pick: it takes the same
    // path `/confirm` does, so the price charged is the one quoted for exactly
    // this release. A missing provider cannot fall through to BT in that case.
    let picked = body
        .candidate
        .as_ref()
        .filter(|value| !value.is_null())
        .cloned();
    let reply = service(move || match body.kind {
        Kind::Show => match picked {
            // Picking a release for a show transfers that release as it is (a
            // season pack or a single episode): there is no per-episode unit to
            // plan, and the request queue is not involved.
            Some(candidate) => {
                let Some(resolve) = cloud.as_ref() else {
                    return Ok(AcquisitionResponse::new(AcquisitionState::Failed)
                        .provider(PROVIDER_CLOUD115)
                        .error("the cloud provider is not available"));
                };
                acquire_show_candidate(&state, resolve, &body, &candidate)
            }
            None => show_step(&state, &user, &body),
        },
        Kind::Movie => {
            let ask = movie_ask(
                &state,
                body.tmdb_id,
                body.title.trim(),
                body.year,
                body.library.as_deref(),
            )?;
            match picked {
                Some(candidate) => {
                    let Some(resolve) = cloud.as_ref() else {
                        return Ok(AcquisitionResponse::new(AcquisitionState::Failed)
                            .provider(PROVIDER_CLOUD115)
                            .error("the cloud provider is not available"));
                    };
                    confirm_movie(&state, resolve, &ask, &candidate)
                }
                None => start_movie(&state, &user, cloud, &ask),
            }
        }
    })
    .await?;
    Ok(Json(reply).into_response())
}

/// Transfer one picked show release. Shows carry no per-episode unit here: the
/// provider's own acquire takes the season and index, prices the unlock the
/// same way, and the transfer lands in the transit folder.
fn acquire_show_candidate(
    state: &SharedState,
    resolve: &Resolver,
    body: &StartBody,
    candidate: &Value,
) -> anyhow::Result<AcquisitionResponse> {
    let Some(normalized) = normalize_candidate(candidate) else {
        return Ok(AcquisitionResponse::new(AcquisitionState::Failed)
            .provider(PROVIDER_CLOUD115)
            .error("candidate is not a valid acquisition candidate"));
    };
    if needs_confirm(&normalized) {
        return Ok(AcquisitionResponse::new(AcquisitionState::NeedsConfirm)
            .provider(PROVIDER_CLOUD115)
            .candidate(candidate.clone())
            .quote(quote_of(&normalized)));
    }
    let season = body
        .season
        .or_else(|| body.seasons.as_ref().and_then(|seasons| seasons.first().copied()))
        .unwrap_or(0);
    // A season pack has no per-episode unit; season 0 marks "the whole season"
    // so the attempt still has an identity the ledger can key on.
    let unit = planner::episode_unit(body.tmdb_id, season, 0);
    planner::ensure_unit(state, &unit, body.title.trim(), body.year)?;
    let attempt = new_attempt(
        &unit,
        PROVIDER_CLOUD115,
        AttemptStatus::Queued,
        Some(candidate.clone()),
        quote_json(quote_of(&normalized)),
    );
    insert_attempt(state, &attempt)?;
    let raw: Value = kroma_module_host::call_point(
        resolve,
        PROVIDER_CLOUD115,
        "acquire",
        &json!({
            "attemptId": &attempt.id,
            "kind": "show",
            "tmdbId": body.tmdb_id,
            "title": body.title.trim(),
            "year": body.year,
            "library": &body.library,
            "season": season,
            "candidate": candidate,
            "confirmed": true,
        }),
    )?;
    let reply = AcquireReply::parse(raw)?;
    let now = jobs::now_ms();
    record_acquire(
        &state.db,
        &attempt.id,
        &reply.job_id,
        reply.points.unwrap_or(0),
        reply.unlock_key.as_deref(),
        now,
    )?;
    db::set_attempt_status(
        &state.db,
        &attempt.id,
        AttemptStatus::Transferring,
        "",
        "",
        None,
        now,
    )?;
    Ok(AcquisitionResponse::new(AcquisitionState::Started)
        .provider(PROVIDER_CLOUD115)
        .attempt(&attempt.id)
        .job(&reply.job_id)
        .candidate(candidate.clone())
        .quote(quote_of(&normalized)))
}

/// `POST /api/acquisition/candidates` body: what the acquisition panel asks for
/// before the caller picks a release.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidatesBody {
    kind: Kind,
    tmdb_id: u64,
    title: String,
    #[serde(default)]
    year: Option<u32>,
    #[serde(default)]
    library: Option<String>,
    /// For a show: which season to list. Absent lists the source's default
    /// (usually the latest); the panel offers a season picker when it matters.
    #[serde(default)]
    season: Option<u32>,
}

/// One row the panel draws. The candidate is flattened: the provider's own
/// fields (index, fileName, link, shareCode, price, size) are lifted out of
/// `extra` to the top level, because the panel reads them and hands the whole
/// object back to `start`/`confirm` — a shape whose fields live one level down
/// would reach the client as an object it cannot name. `extra` is kept as well,
/// so the provider's own reader still finds it on the way back in.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CandidateView {
    /// The flat candidate, as one object: identity fields plus every provider
    /// field, plus the original `extra`.
    #[serde(flatten)]
    fields: serde_json::Map<String, Value>,
    needs_confirm: bool,
    recommended: bool,
}

/// Lift a normalized candidate back into one flat object. The identity fields
/// win on a name clash; everything the provider put in `extra` rides beside
/// them, and `extra` itself is kept verbatim for a re-issued `acquire`.
fn flat_candidate(candidate: &Candidate) -> serde_json::Map<String, Value> {
    let mut flat = match &candidate.extra {
        Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    flat.insert("id".into(), Value::String(candidate.id.clone()));
    flat.insert("provider".into(), Value::String(candidate.provider.clone()));
    flat.insert("title".into(), Value::String(candidate.title.clone()));
    if let Some(size) = candidate.size_bytes {
        flat.insert("sizeBytes".into(), Value::from(size));
    }
    if let Some(points) = candidate.price_points {
        flat.insert("pricePoints".into(), Value::from(points));
    }
    flat.insert("priceKnown".into(), Value::Bool(candidate.price_known));
    flat.insert("grabbable".into(), Value::Bool(candidate.grabbable));
    if let Some(reason) = candidate.reject_reason.clone() {
        flat.insert("rejectReason".into(), Value::String(reason));
    }
    flat.insert("extra".into(), candidate.extra.clone());
    flat
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CandidatesReply {
    /// Which release the automatic path would take, if any.
    recommended_id: Option<String>,
    candidates: Vec<CandidateView>,
    /// Why there is nothing to show (provider missing, not signed in, the
    /// search failed). Empty when the list itself is the answer, even empty.
    blockers: Vec<String>,
    /// The title is already held; the panel shows that instead of a search.
    owned: Option<OwnedState>,
}

/// `POST /api/acquisition/candidates` — the list `/start` would otherwise
/// choose from on its own. Nothing is written, charged or queued here: this is
/// the read half of the acquisition panel, and `/confirm` still performs the
/// grab.
pub async fn candidates(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CandidatesBody>,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    if body.tmdb_id == 0 || body.title.trim().is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "a title needs a TMDB id and a name",
        ));
    }
    let locale = i18n::user_locale(&user);
    let Some(cloud) = point::require(&state, &user, PROVIDER_CLOUD115).ok() else {
        return Err(lerr(locale, StatusCode::NOT_FOUND, "error.moduleDisabled"));
    };
    let reply = service(move || match body.kind {
        Kind::Show => list_show_candidates(&state, &cloud, &body),
        Kind::Movie => {
            let ask = movie_ask(
                &state,
                body.tmdb_id,
                body.title.trim(),
                body.year,
                body.library.as_deref(),
            )?;
            list_candidates(&state, &cloud, &ask)
        }
    })
    .await?;
    Ok(Json(reply).into_response())
}

/// A show's list, straight from the provider: a show has no single unit to
/// place, so the panel is the whole decision. Nothing is written; picking a row
/// is what starts a transfer (`/start` with that candidate).
fn list_show_candidates(
    state: &SharedState,
    cloud: &Resolver,
    body: &CandidatesBody,
) -> anyhow::Result<CandidatesReply> {
    let _ = state;
    let readiness: ProviderReadiness = match kroma_module_host::call_point(
        cloud,
        PROVIDER_CLOUD115,
        "readiness",
        &json!({ "library": &body.library }),
    ) {
        Ok(readiness) => readiness,
        Err(error) => {
            return Ok(CandidatesReply {
                recommended_id: None,
                candidates: Vec::new(),
                blockers: vec![format!("cloud115 is not reachable: {error}")],
                owned: None,
            });
        }
    };
    if !readiness.ready {
        return Ok(CandidatesReply {
            recommended_id: None,
            candidates: Vec::new(),
            blockers: if readiness.blockers.is_empty() {
                vec!["cloud115 is not ready".into()]
            } else {
                readiness.blockers
            },
            owned: None,
        });
    }
    let found: Value = match kroma_module_host::call_point(
        cloud,
        PROVIDER_CLOUD115,
        "search",
        &json!({
            "kind": "show",
            "tmdbId": body.tmdb_id,
            "title": body.title.trim(),
            "year": body.year,
            "library": &body.library,
            "season": body.season.unwrap_or(0),
            // Probe, so a show's panel shows what each release costs too.
            "probe": true,
        }),
    ) {
        Ok(found) => found,
        Err(error) => {
            return Ok(CandidatesReply {
                recommended_id: None,
                candidates: Vec::new(),
                blockers: vec![format!("cloud115 search failed: {error}")],
                owned: None,
            });
        }
    };
    let hits = hits_of(found);
    let recommended_id = best_candidate(&hits).map(|hit| hit.candidate.id.clone());
    let candidates = hits
        .into_iter()
        .map(|hit| CandidateView {
            recommended: recommended_id.as_deref() == Some(hit.candidate.id.as_str()),
            needs_confirm: needs_confirm(&hit.candidate),
            fields: flat_candidate(&hit.candidate),
        })
        .collect();
    Ok(CandidatesReply {
        recommended_id,
        candidates,
        blockers: Vec::new(),
        owned: None,
    })
}

/// Read the provider's list for one film. A provider failure is reported in
/// `blockers` rather than as an HTTP error: the panel must still open, if only
/// to say why there is nothing to pick.
fn list_candidates(
    state: &SharedState,
    cloud: &Resolver,
    ask: &MovieAsk,
) -> anyhow::Result<CandidatesReply> {
    let unit = planner::movie_unit(ask.tmdb_id);
    planner::ensure_unit(state, &unit, &ask.title, ask.year)?;
    let plan = planner::plan(state, &unit, ask.policy)?;
    if plan.next == NextAction::Complete {
        return Ok(CandidatesReply {
            recommended_id: None,
            candidates: Vec::new(),
            blockers: Vec::new(),
            owned: Some(plan.owned),
        });
    }
    let hits = match cloud_search(cloud, &unit, ask) {
        Ok(hits) => hits,
        Err(why) => {
            return Ok(CandidatesReply {
                recommended_id: None,
                candidates: Vec::new(),
                blockers: vec![why],
                owned: None,
            });
        }
    };
    let recommended_id = best_candidate(&hits).map(|hit| hit.candidate.id.clone());
    let candidates = hits
        .into_iter()
        .map(|hit| CandidateView {
            recommended: recommended_id.as_deref() == Some(hit.candidate.id.as_str()),
            needs_confirm: needs_confirm(&hit.candidate),
            fields: flat_candidate(&hit.candidate),
        })
        .collect();
    Ok(CandidatesReply {
        recommended_id,
        candidates,
        blockers: Vec::new(),
        owned: None,
    })
}

/// `POST /api/acquisition/confirm` — the caller accepted the quote of the
/// candidate a search returned. Re-issues `cloud115/acquire` with
/// `confirmed: true` for that attempt, and never charges a second time when the
/// attempt already has a job id for the same resource.
pub async fn confirm(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Json(body): Json<ConfirmBody>,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    if body.tmdb_id == 0 || body.title.trim().is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "a title needs a TMDB id and a name",
        ));
    }
    if normalize_candidate(&body.candidate).is_none() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "candidate is not a valid acquisition candidate",
        ));
    }
    let locale = i18n::user_locale(&user);
    // Unlike `/start`, a missing provider cannot fall through to anything: the
    // caller is confirming a specific cloud grab, so it errors clearly.
    let Some(cloud) = point::require(&state, &user, PROVIDER_CLOUD115).ok() else {
        return Err(lerr(locale, StatusCode::NOT_FOUND, "error.moduleDisabled"));
    };
    let reply = service(move || match body.kind {
        // A show's candidate is confirmed against the same transfer path its
        // `/start` took: no request queue, no per-episode plan.
        Kind::Show => {
            let start = StartBody {
                kind: Kind::Show,
                tmdb_id: body.tmdb_id,
                title: body.title.clone(),
                year: body.year,
                seasons: body.season.map(|season| vec![season]),
                episodes: None,
                library: body.library.clone(),
                season: body.season,
                candidate: Some(body.candidate.clone()),
            };
            acquire_show_candidate(&state, &cloud, &start, &body.candidate)
        }
        Kind::Movie => {
            let ask = movie_ask(
                &state,
                body.tmdb_id,
                body.title.trim(),
                body.year,
                body.library.as_deref(),
            )?;
            confirm_movie(&state, &cloud, &ask, &body.candidate)
        }
    })
    .await?;
    Ok(Json(reply).into_response())
}

/// `GET /api/acquisition/status?attemptId=…` — one attempt row, the unit's
/// locations, and (for a 115 job) the provider's latest status. Read-only apart
/// from recording the status the provider just reported.
pub async fn status(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Query(query): Query<StatusQuery>,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    let id = query.attempt_id.trim().to_string();
    if id.is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "attemptId is required"));
    }
    let locale = i18n::user_locale(&user);
    let cloud = point::require(&state, &user, PROVIDER_CLOUD115).ok();
    let view = blocking(move || read_status(&state, cloud, &id, jobs::now_ms())).await?;
    match view {
        Some(view) => Ok(Json(view).into_response()),
        None => Err(lerr(locale, StatusCode::NOT_FOUND, "error.itemNotFound")),
    }
}

/// `GET /api/acquisition/tasks` — every cloud acquisition the server has run,
/// newest first. The request queue only knows BT-path work; a 115 grab has no
/// request row, so the acquisition centre's "tasks" tab would miss it entirely
/// without this.
pub async fn tasks(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
) -> Result<Response, Response> {
    require(&user, Permission::RequestsCreate)?;
    let rows = blocking(move || {
        let conn = state.db.get()?;
        let mut attempts = db::attempts_by_provider(&conn, PROVIDER_CLOUD115)?;
        // Newest first, and bounded: this is a recent-activity list.
        attempts.reverse();
        attempts.truncate(100);
        // The unit row knows what the title is; an attempt row alone says only
        // `movie:1381071`, which reads as an id, not a film.
        let titles: Vec<(String, String)> = {
            let mut stmt = conn.prepare("SELECT unit_key, title FROM media_units")?;
            let mapped = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            mapped.filter_map(Result::ok).collect()
        };
        let named: Vec<Value> = attempts
            .iter()
            .map(|attempt| {
                let title = titles
                    .iter()
                    .find(|(key, _)| key == &attempt.unit_key)
                    .map(|(_, title)| title.clone())
                    .unwrap_or_default();
                serde_json::json!({
                    "id": attempt.id,
                    "unitKey": attempt.unit_key,
                    "title": title,
                    "provider": attempt.provider,
                    "status": attempt.status,
                    "errorCode": attempt.error_code,
                    "errorMessage": attempt.error_message,
                    "jobId": attempt.job_id,
                    "costPoints": attempt.cost_points,
                    "quote": attempt.quote,
                    "createdAt": attempt.created_at,
                    "updatedAt": attempt.updated_at,
                    "finishedAt": attempt.finished_at,
                })
            })
            .collect();
        Ok::<_, anyhow::Error>(named)
    })
    .await?;
    Ok(Json(serde_json::json!({ "attempts": rows })).into_response())
}

/// The identity of a film ask, passed down the provider flow.
struct MovieAsk {
    tmdb_id: u64,
    title: String,
    year: Option<u32>,
    library: Option<String>,
    library_id: Option<String>,
    policy: AcquisitionPolicy,
}

fn is_video_library(library: &crate::services::settings::LibraryDef) -> bool {
    library.kind.is_empty() || matches!(library.kind.as_str(), "movies" | "mixed")
}

fn movie_ask(
    state: &SharedState,
    tmdb_id: u64,
    title: &str,
    year: Option<u32>,
    requested_library: Option<&str>,
) -> anyhow::Result<MovieAsk> {
    let libraries = crate::services::settings::library_defs(&state.settings, &state.config)?;
    let preferred = state.settings.get_str("acqMovieLibrary", "Auto");
    let default_target = libraries
        .iter()
        .find(|library| {
            preferred != "Auto" && library.name == preferred && is_video_library(library)
        })
        .or_else(|| libraries.iter().find(|library| library.kind == "movies"))
        .or_else(|| libraries.iter().find(|library| library.kind == "mixed"))
        .or_else(|| libraries.iter().find(|library| library.kind.is_empty()));
    let selected =
        if let Some(requested) = requested_library.map(str::trim).filter(|v| !v.is_empty()) {
            let library = libraries
                .iter()
                .find(|library| library.id == requested || library.name == requested)
                .ok_or_else(|| anyhow::anyhow!("unknown acquisition library {requested:?}"))?;
            anyhow::ensure!(
                is_video_library(library),
                "library {:?} does not accept movie acquisitions",
                library.name
            );
            anyhow::ensure!(
                default_target.is_some_and(|target| target.id == library.id),
                "select {:?} as the movie import library before acquiring into it",
                library.name
            );
            Some(library)
        } else {
            default_target
        };
    let selected =
        selected.ok_or_else(|| anyhow::anyhow!("no movie or mixed media library is configured"))?;
    let mode = crate::services::settings::library_acquisition(&state.settings, &selected.id);
    Ok(MovieAsk {
        tmdb_id,
        title: title.to_string(),
        year,
        library: Some(selected.name.clone()),
        library_id: Some(selected.id.clone()),
        policy: mode.policy(),
    })
}

/// Create (or merge) a show request and let the existing BT path take it. No
/// attempt row: the request + wanted ledgers already describe that work.
fn show_step(
    state: &SharedState,
    user: &User,
    body: &StartBody,
) -> anyhow::Result<AcquisitionResponse> {
    let ask = CreateRequestBody {
        kind: RequestKind::Show,
        tmdb_id: body.tmdb_id,
        seasons: body.seasons.clone(),
        episodes: body.episodes.clone(),
    };
    let request = requests::create_request(state, user, &ask)?;
    Ok(show_reply(
        request.status,
        &request.id,
        request.note.as_deref(),
    ))
}

fn show_reply(status: RequestStatus, request_id: &str, note: Option<&str>) -> AcquisitionResponse {
    let mut reply = AcquisitionResponse::new(request_state(status)).request(request_id);
    match status {
        RequestStatus::Pending => {}
        RequestStatus::Denied | RequestStatus::Failed => {
            reply = reply.error(
                note.filter(|message| !message.trim().is_empty())
                    .unwrap_or("the show request was not accepted"),
            );
        }
        _ => reply = reply.provider(PROVIDER_BT),
    }
    reply
}

/// The film flow: materialize the unit, answer what is already held, then walk
/// the policy's provider order.
fn start_movie(
    state: &SharedState,
    user: &User,
    cloud: Option<Resolver>,
    ask: &MovieAsk,
) -> anyhow::Result<AcquisitionResponse> {
    let unit = planner::movie_unit(ask.tmdb_id);
    planner::ensure_unit(state, &unit, &ask.title, ask.year)?;
    let plan = planner::plan(state, &unit, ask.policy)?;

    if let Some(active) = plan.active_attempt.as_ref() {
        return Ok(resume(active));
    }
    match plan.next {
        NextAction::Complete => return Ok(owned_reply(plan.owned, plan.next)),
        NextAction::UploadToCloud | NextAction::EvictLocalStaging => {
            let Some(resolve) = cloud.as_ref() else {
                return Ok(AcquisitionResponse::new(AcquisitionState::Failed)
                    .provider(PROVIDER_CLOUD115)
                    .blockers(vec!["cloud115: no module answers this provider".into()])
                    .error("the library needs a cloud copy but 115 is unavailable"));
            };
            return start_local_cloud_upload(state, resolve, &unit, ask);
        }
        NextAction::DownloadToLocal => {
            return bt_step(
                state,
                user,
                &unit,
                ask,
                vec!["a cloud copy exists; acquiring a local copy for this library".into()],
            );
        }
        NextAction::Acquire => {}
    }

    let mut blockers = Vec::new();
    for provider in &plan.providers {
        match provider.as_str() {
            PROVIDER_CLOUD115 => {
                if plan.next == NextAction::UploadToCloud {
                    let Some(resolve) = cloud.as_ref() else {
                        return Ok(AcquisitionResponse::new(AcquisitionState::Failed)
                            .provider(PROVIDER_CLOUD115)
                            .blockers(vec!["cloud115: no module answers this provider".into()])
                            .error("the library needs a cloud copy but 115 is unavailable"));
                    };
                    return start_local_cloud_upload(state, resolve, &unit, ask);
                }
                match cloud_step(state, cloud.as_ref(), &unit, ask)? {
                    ProviderStep::Reply(reply) => return Ok(reply),
                    ProviderStep::Fallback(why) => blockers = why,
                }
            }
            PROVIDER_BT => return bt_step(state, user, &unit, ask, blockers),
            _ => {}
        }
    }
    Ok(AcquisitionResponse::new(AcquisitionState::Failed)
        .error("no acquisition provider is available for this title"))
}

/// Try the cloud provider. `Fallback` carries why it did not answer so the BT
/// reply can explain itself; it is never an error, because a missing provider is
/// a supported state.
fn cloud_step(
    state: &SharedState,
    cloud: Option<&Resolver>,
    unit: &MediaUnitRef,
    ask: &MovieAsk,
) -> anyhow::Result<ProviderStep> {
    let Some(resolve) = cloud else {
        return Ok(ProviderStep::Fallback(vec![
            "cloud115: no module answers this provider".into(),
        ]));
    };
    let hits = match cloud_search(resolve, unit, ask) {
        Ok(hits) => hits,
        Err(why) => return Ok(ProviderStep::Fallback(vec![why])),
    };
    let Some(best) = best_candidate(&hits) else {
        return Ok(ProviderStep::Fallback(vec![
            "cloud115: no grabbable candidate".into(),
        ]));
    };
    let quote = quote_of(&best.candidate);
    let candidate = best.raw.clone();

    if needs_confirm(&best.candidate) {
        let attempt = new_attempt(
            unit,
            PROVIDER_CLOUD115,
            AttemptStatus::WaitingConfirm,
            Some(candidate.clone()),
            quote_json(quote),
        );
        insert_attempt(state, &attempt)?;
        return Ok(ProviderStep::Reply(
            AcquisitionResponse::new(AcquisitionState::NeedsConfirm)
                .provider(PROVIDER_CLOUD115)
                .attempt(&attempt.id)
                .candidate(candidate)
                .quote(quote),
        ));
    }

    // Free or already unlocked: the start click is the confirmation. The row is
    // written BEFORE the provider call, so a concurrent start loses the unique
    // active-attempt race instead of charging twice.
    let attempt = new_attempt(
        unit,
        PROVIDER_CLOUD115,
        AttemptStatus::Queued,
        Some(candidate.clone()),
        quote_json(quote),
    );
    insert_attempt(state, &attempt)?;
    Ok(ProviderStep::Reply(acquire_and_record(
        state,
        resolve,
        unit,
        ask,
        &attempt.id,
        &candidate,
        quote,
    )?))
}

/// The BT fallback: create/approve the request, then record one `queued` attempt
/// so `/status` can join the two ledgers.
fn bt_step(
    state: &SharedState,
    user: &User,
    unit: &MediaUnitRef,
    ask: &MovieAsk,
    blockers: Vec<String>,
) -> anyhow::Result<AcquisitionResponse> {
    let body = CreateRequestBody {
        kind: RequestKind::Movie,
        tmdb_id: ask.tmdb_id,
        seasons: None,
        episodes: None,
    };
    let mut request = requests::create_request(state, user, &body)?;
    // Starting an acquisition is an ask to begin: a manager (or an
    // auto-approving requester) clears review right here. Everyone else keeps
    // the normal pending-review path.
    if request.status == RequestStatus::Pending && may_approve(user) {
        request = requests::approve_request(state, &request.id, Some(&user.id))?;
    }
    let target = ask
        .library_id
        .as_ref()
        .map(|library_id| json!({ "libraryId": library_id }));
    let mut attempt = new_attempt(unit, PROVIDER_BT, AttemptStatus::Queued, target, None);
    attempt.request_id = Some(request.id.clone());
    insert_attempt(state, &attempt)?;
    Ok(AcquisitionResponse::new(request_state(request.status))
        .provider(PROVIDER_BT)
        .request(&request.id)
        .attempt(&attempt.id)
        .blockers(blockers))
}

/// `requests.auto` is the existing rule for skipping review, and a
/// `requests.manage` holder reviewing their own ask is the same decision.
fn start_local_cloud_upload(
    state: &SharedState,
    resolve: &Resolver,
    unit: &MediaUnitRef,
    ask: &MovieAsk,
) -> anyhow::Result<AcquisitionResponse> {
    let path = {
        let conn = state.db.get()?;
        db::movie_files_by_tmdb(&conn, ask.tmdb_id, ask.library_id.as_deref())?
            .into_iter()
            .map(|file| file.path)
            .find(|path| {
                let path_ref = std::path::Path::new(path);
                path_ref.is_file()
                    && path_ref
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| {
                            matches!(
                                ext.to_ascii_lowercase().as_str(),
                                "mkv"
                                    | "mp4"
                                    | "avi"
                                    | "mov"
                                    | "wmv"
                                    | "webm"
                                    | "m4v"
                                    | "mpeg"
                                    | "mpg"
                                    | "ts"
                                    | "m2ts"
                            )
                        })
            })
    };
    let Some(path) = path else {
        return Ok(AcquisitionResponse::new(AcquisitionState::Failed)
            .provider(PROVIDER_CLOUD115)
            .error("the library is missing a local video file to upload"));
    };

    let candidate = json!({
        "path": path,
        "deleteLocal": ask.policy.evict_after_cloud_verified,
    });
    let attempt = new_attempt(
        unit,
        "cloud115_upload",
        AttemptStatus::Queued,
        Some(candidate.clone()),
        None,
    );
    insert_attempt(state, &attempt)?;
    #[derive(Debug, Default, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UploadStarted {
        job_id: String,
    }
    let started = kroma_module_host::call_point::<_, UploadStarted>(
        resolve,
        PROVIDER_CLOUD115,
        "upload",
        &json!({
            "path": path,
            "deleteLocal": ask.policy.evict_after_cloud_verified,
        }),
    );
    match started {
        Ok(reply) if !reply.job_id.trim().is_empty() => {
            let now = jobs::now_ms();
            db::set_attempt_job(&state.db, &attempt.id, &reply.job_id, now)?;
            db::set_attempt_status(
                &state.db,
                &attempt.id,
                AttemptStatus::Transferring,
                "",
                "",
                None,
                now,
            )?;
            let _ = state.jobs.trigger(
                state.clone(),
                crate::services::jobs::JobKey("acquisition.staging"),
                "acquisition-upload",
            );
            Ok(AcquisitionResponse::new(AcquisitionState::Started)
                .provider(PROVIDER_CLOUD115)
                .attempt(&attempt.id)
                .job(&reply.job_id)
                .candidate(candidate))
        }
        Ok(_) => {
            let now = jobs::now_ms();
            db::set_attempt_status(
                &state.db,
                &attempt.id,
                AttemptStatus::Failed,
                "provider_no_job",
                "115 上传模块没有返回任务号；本地文件保留",
                Some(now),
                now,
            )?;
            Ok(AcquisitionResponse::new(AcquisitionState::Failed)
                .provider(PROVIDER_CLOUD115)
                .attempt(&attempt.id)
                .error("115 upload provider did not return a job id"))
        }
        Err(error) => {
            let now = jobs::now_ms();
            db::set_attempt_status(
                &state.db,
                &attempt.id,
                AttemptStatus::Failed,
                "provider_error",
                &error.to_string(),
                Some(now),
                now,
            )?;
            Ok(AcquisitionResponse::new(AcquisitionState::Failed)
                .provider(PROVIDER_CLOUD115)
                .attempt(&attempt.id)
                .error(error.to_string()))
        }
    }
}

fn may_approve(user: &User) -> bool {
    user.can(Permission::RequestsAuto) || user.can(Permission::RequestsManage)
}

/// The confirmation flow. The active attempt is the authority: when it already
/// has a job id for this exact candidate the running job is handed back and
/// nothing is charged again; otherwise `acquire` is re-issued for the SAME
/// attempt id.
fn confirm_movie(
    state: &SharedState,
    resolve: &Resolver,
    ask: &MovieAsk,
    offered: &Value,
) -> anyhow::Result<AcquisitionResponse> {
    let Some(candidate) = normalize_candidate(offered) else {
        anyhow::bail!("candidate is not a valid acquisition candidate");
    };
    let unit = planner::movie_unit(ask.tmdb_id);
    planner::ensure_unit(state, &unit, &ask.title, ask.year)?;
    let plan = planner::plan(state, &unit, ask.policy)?;
    if plan.next != NextAction::Acquire {
        return Ok(owned_reply(plan.owned, plan.next));
    }

    if let Some(active) = plan.active_attempt.as_ref() {
        let stored = active
            .candidate
            .as_ref()
            .filter(|stored| same_candidate(stored, offered));
        let Some(stored) = stored else {
            return Ok(AcquisitionResponse::new(AcquisitionState::Failed)
                .provider(&active.provider)
                .attempt(&active.id)
                .error("a different acquisition for this title is already running"));
        };
        if active.provider != PROVIDER_CLOUD115 {
            return Ok(AcquisitionResponse::new(AcquisitionState::Failed)
                .provider(&active.provider)
                .attempt(&active.id)
                .error("another provider is already acquiring this title"));
        }
        if !active.job_id.is_empty() {
            // Already paid: same attempt, same resource, so hand back its job.
            if active.status == AttemptStatus::WaitingConfirm {
                let now = jobs::now_ms();
                db::set_attempt_status(
                    &state.db,
                    &active.id,
                    AttemptStatus::Transferring,
                    "",
                    "",
                    None,
                    now,
                )?;
            }
            return Ok(AcquisitionResponse::new(AcquisitionState::Started)
                .provider(PROVIDER_CLOUD115)
                .attempt(&active.id)
                .job(&active.job_id)
                .candidate(stored.clone())
                .quote(stored_quote(active, &candidate)));
        }
        // No job yet: the stored candidate is what the provider quoted, never
        // the (client-supplied) copy, so a replayed body cannot lower the price.
        return acquire_and_record(
            state,
            resolve,
            &unit,
            ask,
            &active.id,
            stored,
            stored_quote(active, &candidate),
        );
    }

    let attempt = new_attempt(
        &unit,
        PROVIDER_CLOUD115,
        AttemptStatus::Queued,
        Some(offered.clone()),
        quote_json(quote_of(&candidate)),
    );
    insert_attempt(state, &attempt)?;
    acquire_and_record(
        state,
        resolve,
        &unit,
        ask,
        &attempt.id,
        offered,
        quote_of(&candidate),
    )
}

/// Call `acquire` and record what the provider answered. A provider failure is
/// an answer, not an HTTP error: the attempt is marked failed and the reply
/// carries the reason.
fn acquire_and_record(
    state: &SharedState,
    resolve: &Resolver,
    unit: &MediaUnitRef,
    ask: &MovieAsk,
    attempt_id: &str,
    candidate: &Value,
    quote: QuoteView,
) -> anyhow::Result<AcquisitionResponse> {
    let now = jobs::now_ms();
    match acquire_at(resolve, attempt_id, unit, ask, candidate, true) {
        Ok(reply) => {
            let cost = reply.points.or(quote.points).unwrap_or(0);
            record_acquire(
                &state.db,
                attempt_id,
                &reply.job_id,
                cost,
                reply.unlock_key.as_deref(),
                now,
            )?;
            db::set_attempt_status(
                &state.db,
                attempt_id,
                AttemptStatus::Transferring,
                "",
                "",
                None,
                now,
            )?;
            Ok(AcquisitionResponse::new(AcquisitionState::Started)
                .provider(PROVIDER_CLOUD115)
                .attempt(attempt_id)
                .job(&reply.job_id)
                .candidate(candidate.clone())
                .quote(quote))
        }
        Err(error) => {
            tracing::warn!(
                attempt = attempt_id,
                error = %format!("{error:#}"),
                "cloud115 acquire failed"
            );
            let message = format!("{error}");
            db::set_attempt_status(
                &state.db,
                attempt_id,
                AttemptStatus::Failed,
                "provider_error",
                &message,
                Some(now),
                now,
            )?;
            Ok(AcquisitionResponse::new(AcquisitionState::Failed)
                .provider(PROVIDER_CLOUD115)
                .attempt(attempt_id)
                .error(message))
        }
    }
}

fn acquire_at(
    resolve: &Resolver,
    attempt_id: &str,
    unit: &MediaUnitRef,
    ask: &MovieAsk,
    candidate: &Value,
    confirmed: bool,
) -> anyhow::Result<AcquireReply> {
    let reply: Value = kroma_module_host::call_point(
        resolve,
        PROVIDER_CLOUD115,
        "acquire",
        &json!({
            "attemptId": attempt_id,
            "unitKey": unit.key(),
            "kind": "movie",
            "tmdbId": ask.tmdb_id,
            "title": &ask.title,
            "year": ask.year,
            "library": &ask.library,
            "candidate": candidate,
            "confirmed": confirmed,
        }),
    )?;
    AcquireReply::parse(reply)
}

/// Read the provider's readiness, then its candidates. Any failure is a reason
/// string, never an error: `/start` turns it into a BT fallback.
fn cloud_search(
    resolve: &Resolver,
    unit: &MediaUnitRef,
    ask: &MovieAsk,
) -> Result<Vec<Hit>, String> {
    let readiness: ProviderReadiness = kroma_module_host::call_point(
        resolve,
        PROVIDER_CLOUD115,
        "readiness",
        &json!({ "library": &ask.library }),
    )
    .map_err(|error| format!("cloud115 is not reachable: {error}"))?;
    if !readiness.ready {
        let why = readiness.blockers.join("; ");
        return Err(if why.is_empty() {
            "cloud115 is not ready".to_string()
        } else {
            why
        });
    }
    let found: Value = kroma_module_host::call_point(
        resolve,
        PROVIDER_CLOUD115,
        "search",
        &json!({
            "kind": "movie",
            "tmdbId": ask.tmdb_id,
            "title": &ask.title,
            "year": ask.year,
            "library": &ask.library,
            "unitKey": unit.key(),
            // Probe: open each candidate's confirmation page (costs no points)
            // so the panel can show what unlocking really costs. Without it the
            // source reports `price: null` and a paid release reads as unknown.
            "probe": true,
        }),
    )
    .map_err(|error| format!("cloud115 search failed: {error}"))?;
    Ok(hits_of(found))
}

/// What one attempt row answers for `/start` and `/confirm` while it is live.
fn resume(attempt: &AcquisitionAttempt) -> AcquisitionResponse {
    let mut reply = AcquisitionResponse::new(resume_state(attempt))
        .provider(&attempt.provider)
        .attempt(&attempt.id);
    if !attempt.job_id.is_empty() {
        reply = reply.job(&attempt.job_id);
    }
    if let Some(request_id) = attempt.request_id.as_deref() {
        reply = reply.request(request_id);
    }
    if let Some(candidate) = attempt.candidate.clone() {
        reply = reply.candidate(candidate);
    }
    if let Some(quote) = attempt
        .quote
        .as_ref()
        .and_then(|value| serde_json::from_value::<QuoteView>(value.clone()).ok())
    {
        reply = reply.quote(quote);
    }
    reply
}

/// A live 115 attempt with no job id has not been charged as far as the core
/// knows (the provider's reply was lost, or its call failed midway), so the user
/// is asked to confirm again and `/confirm` re-issues `acquire` for the SAME
/// attempt id rather than a second attempt.
fn resume_state(attempt: &AcquisitionAttempt) -> AcquisitionState {
    if attempt.provider == PROVIDER_CLOUD115 && attempt.job_id.is_empty() {
        return AcquisitionState::NeedsConfirm;
    }
    state_of(attempt.status)
}

/// `/status`: one attempt, its unit's locations, and the provider's latest word
/// on a running 115 job.
fn read_status(
    state: &SharedState,
    cloud: Option<Resolver>,
    attempt_id: &str,
    now: i64,
) -> anyhow::Result<Option<StatusView>> {
    let conn = state.db.get()?;
    // The ledger API is keyed by unit, and an attempt id is all `/status` gets;
    // one column read bridges the two without a second row mapper here.
    let unit_key: Option<String> = {
        use rusqlite::OptionalExtension;
        conn.query_row(
            "SELECT unit_key FROM acquisition_attempts WHERE id = ?1",
            [attempt_id],
            |row| row.get(0),
        )
        .optional()?
    };
    let Some(unit_key) = unit_key else {
        return Ok(None);
    };
    let Some(mut attempt) = db::attempts_for_unit(&conn, &unit_key)?
        .into_iter()
        .find(|one| one.id == attempt_id)
    else {
        return Ok(None);
    };
    let locations = db::list_media_locations(&conn, &unit_key)?;
    let unit = db::get_media_unit(&conn, &unit_key)?;
    drop(conn);

    let owned = match unit {
        Some(unit) => planner::owned(state, &unit.reference())?,
        None => OwnedState::default(),
    };
    // One point call at most, and only for a job that is still moving: /status
    // must never turn into a scan.
    if attempt.provider == PROVIDER_CLOUD115
        && !attempt.job_id.is_empty()
        && attempt.status.is_active()
    {
        if let Some(resolve) = cloud.as_ref() {
            apply_point_status(state, resolve, &mut attempt, now)?;
        }
    }
    Ok(Some(StatusView {
        state: state_of(attempt.status),
        status: attempt.status,
        attempt,
        locations,
        owned,
    }))
}

/// Record the provider's status on the attempt. An unreachable provider leaves
/// the stored row untouched: `failed to ask` is not `failed`.
fn apply_point_status(
    state: &SharedState,
    resolve: &Resolver,
    attempt: &mut AcquisitionAttempt,
    now: i64,
) -> anyhow::Result<()> {
    let reply = match status_at(resolve, attempt) {
        Ok(reply) => reply,
        Err(error) => {
            tracing::debug!(
                attempt = %attempt.id,
                error = %format!("{error:#}"),
                "cloud115 status unavailable"
            );
            return Ok(());
        }
    };
    let Some(mapped) = status_from_point(&reply.status) else {
        return Ok(());
    };
    if mapped == attempt.status {
        return Ok(());
    }
    let failed = mapped == AttemptStatus::Failed;
    let code = if failed && !reply.error_code.is_empty() {
        reply.error_code.clone()
    } else if failed {
        "provider_error".to_string()
    } else {
        String::new()
    };
    let message = if failed {
        reply.error_message.clone()
    } else {
        String::new()
    };
    let finished = if mapped.is_active() { None } else { Some(now) };
    db::set_attempt_status(
        &state.db,
        &attempt.id,
        mapped,
        &code,
        &message,
        finished,
        now,
    )?;
    attempt.status = mapped;
    attempt.error_code = code;
    attempt.error_message = message;
    attempt.updated_at = now;
    attempt.finished_at = finished;
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PointStatus {
    #[serde(default, alias = "state")]
    status: String,
    #[serde(default)]
    error_code: String,
    #[serde(default, alias = "error")]
    error_message: String,
}

fn status_at(resolve: &Resolver, attempt: &AcquisitionAttempt) -> anyhow::Result<PointStatus> {
    kroma_module_host::call_point(
        resolve,
        PROVIDER_CLOUD115,
        "status",
        &json!({
            "attemptId": attempt.id,
            "jobId": attempt.job_id,
            "unitKey": attempt.unit_key,
        }),
    )
}

/// What the provider's job vocabulary maps onto in the ledger. Unknown words are
/// left alone rather than guessed at.
fn status_from_point(status: &str) -> Option<AttemptStatus> {
    Some(match status.trim().to_ascii_lowercase().as_str() {
        "queued" | "pending" | "waiting" => AttemptStatus::Queued,
        "searching" => AttemptStatus::Searching,
        "transferring" | "copying" | "running" | "active" | "uploading" => {
            AttemptStatus::Transferring
        }
        "downloading" => AttemptStatus::Downloading,
        "importing" => AttemptStatus::Importing,
        "verifying" => AttemptStatus::Verifying,
        "done" | "complete" | "completed" | "success" | "succeeded" | "verified" => {
            AttemptStatus::Done
        }
        "failed" | "error" => AttemptStatus::Failed,
        "cancelled" | "canceled" => AttemptStatus::Cancelled,
        "needs_review" => AttemptStatus::NeedsReview,
        _ => return None,
    })
}

fn state_of(status: AttemptStatus) -> AcquisitionState {
    match status {
        AttemptStatus::Queued
        | AttemptStatus::Searching
        | AttemptStatus::CandidateReady
        | AttemptStatus::Transferring
        | AttemptStatus::Downloading
        | AttemptStatus::Importing
        | AttemptStatus::Verifying => AcquisitionState::Started,
        AttemptStatus::WaitingConfirm => AcquisitionState::NeedsConfirm,
        AttemptStatus::Done => AcquisitionState::Done,
        AttemptStatus::Failed => AcquisitionState::Failed,
        AttemptStatus::Cancelled => AcquisitionState::Cancelled,
        AttemptStatus::NeedsReview => AcquisitionState::PendingReview,
    }
}

fn request_state(status: RequestStatus) -> AcquisitionState {
    match status {
        RequestStatus::Pending => AcquisitionState::PendingReview,
        RequestStatus::Denied | RequestStatus::Failed => AcquisitionState::Failed,
        _ => AcquisitionState::Started,
    }
}

fn owned_reply(owned: OwnedState, next: NextAction) -> AcquisitionResponse {
    AcquisitionResponse::new(AcquisitionState::Owned)
        .owned(owned)
        .next(next)
}

/// One search hit: the normalized candidate plus the provider's exact JSON, so
/// `/confirm` can be handed back the same object the client saw.
struct Hit {
    candidate: Candidate,
    raw: Value,
}

/// What a provider may leave out of a candidate. `priceKnown` defaults to
/// "there is a price", `grabbable` to yes; both are stated in the point contract
/// above, and the defaults keep an older provider usable.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CandidateWire {
    #[serde(default)]
    id: String,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    size_bytes: Option<i64>,
    #[serde(default)]
    quality: Option<Value>,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    price_points: Option<u32>,
    #[serde(default)]
    price_known: Option<bool>,
    #[serde(default)]
    grabbable: Option<bool>,
    #[serde(default)]
    reject_reason: Option<String>,
    #[serde(default)]
    extra: Value,
}

fn normalize_candidate(raw: &Value) -> Option<Candidate> {
    let wire: CandidateWire = serde_json::from_value(raw.clone()).ok()?;
    let id = wire.id.trim().to_string();
    if id.is_empty() {
        return None;
    }
    let provider = match wire.provider.trim() {
        "" => PROVIDER_CLOUD115.to_string(),
        named => named.to_string(),
    };
    Some(Candidate {
        id,
        provider,
        title: wire.title.unwrap_or_default(),
        size_bytes: wire.size_bytes,
        quality: wire.quality,
        languages: wire.languages,
        seeders: wire.seeders,
        price_points: wire.price_points,
        price_known: wire.price_known.unwrap_or(wire.price_points.is_some()),
        grabbable: wire.grabbable.unwrap_or(true),
        reject_reason: wire.reject_reason.filter(|reason| !reason.is_empty()),
        extra: if wire.extra.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            wire.extra
        },
    })
}

/// Providers answer with the list itself or wrapped under `candidates`; both
/// read the same.
fn hits_of(value: Value) -> Vec<Hit> {
    let items = match value {
        Value::Array(items) => items,
        Value::Object(mut object) => match object.remove("candidates") {
            Some(Value::Array(items)) => items,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|raw| normalize_candidate(&raw).map(|candidate| Hit { candidate, raw }))
        .collect()
}

/// The best grabbable offer: the provider's own `extra.score` first (quality),
/// then a known price over an unknown one, then the cheaper of two known prices,
/// then seeders, then size.
fn best_candidate(hits: &[Hit]) -> Option<&Hit> {
    hits.iter()
        .filter(|hit| hit.candidate.grabbable)
        .filter(|hit| {
            hit.candidate
                .reject_reason
                .as_deref()
                .is_none_or(|reason| reason.is_empty())
        })
        .max_by_key(|hit| candidate_rank(&hit.candidate))
}

fn candidate_rank(candidate: &Candidate) -> (i64, i64, i64, i64, i64) {
    let score = candidate
        .extra
        .get("score")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let (known, price) = match (candidate.price_known, candidate.price_points) {
        (true, Some(points)) => (1, -i64::from(points)),
        _ => (0, 0),
    };
    (
        score,
        known,
        price,
        i64::from(candidate.seeders.unwrap_or(0)),
        candidate.size_bytes.unwrap_or(0),
    )
}

/// A null/unknown price or any price above zero is the caller's decision to
/// make. A known-free or already-unlocked resource is not.
fn needs_confirm(candidate: &Candidate) -> bool {
    if already_unlocked(candidate) {
        return false;
    }
    // Only a price the provider stated AND set to zero skips the click: a null
    // price with `priceKnown: true` is a contradiction, still worth asking.
    !(candidate.price_known && candidate.price_points == Some(0))
}

fn already_unlocked(candidate: &Candidate) -> bool {
    candidate
        .extra
        .get("unlocked")
        .and_then(Value::as_bool)
        .or_else(|| {
            candidate
                .extra
                .get("alreadyUnlocked")
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
}

fn same_candidate(stored: &Value, offered: &Value) -> bool {
    match (normalize_candidate(stored), normalize_candidate(offered)) {
        (Some(stored), Some(offered)) => {
            stored.id == offered.id && stored.provider == offered.provider
        }
        _ => false,
    }
}

/// A provider's answer to `acquire`. A bare string is read as the job id, which
/// is what the simplest provider returns.
#[derive(Debug, Default)]
struct AcquireReply {
    job_id: String,
    points: Option<u32>,
    unlock_key: Option<String>,
}

impl AcquireReply {
    fn parse(value: Value) -> anyhow::Result<Self> {
        match value {
            Value::String(job_id) if !job_id.trim().is_empty() => Ok(Self {
                job_id,
                ..Self::default()
            }),
            Value::Object(_) => {
                let wire: AcquireWire = serde_json::from_value(value)?;
                let job_id = if wire.job_id.trim().is_empty() {
                    wire.job.unwrap_or_default()
                } else {
                    wire.job_id
                };
                anyhow::ensure!(!job_id.trim().is_empty(), "the provider returned no job id");
                Ok(Self {
                    job_id,
                    points: wire.points,
                    unlock_key: wire.unlock_key,
                })
            }
            _ => anyhow::bail!("unexpected reply from the provider"),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcquireWire {
    #[serde(default)]
    job_id: String,
    #[serde(default)]
    job: Option<String>,
    #[serde(default)]
    points: Option<u32>,
    #[serde(default)]
    unlock_key: Option<String>,
}

/// The price answer the client confirms. `points: null` means "the provider did
/// not say", which is not the same as free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteView {
    points: Option<u32>,
    price_known: bool,
}

fn quote_of(candidate: &Candidate) -> QuoteView {
    QuoteView {
        points: candidate.price_points,
        price_known: candidate.price_known,
    }
}

fn quote_json(quote: QuoteView) -> Option<Value> {
    serde_json::to_value(quote).ok()
}

fn stored_quote(attempt: &AcquisitionAttempt, candidate: &Candidate) -> QuoteView {
    attempt
        .quote
        .as_ref()
        .and_then(|value| serde_json::from_value::<QuoteView>(value.clone()).ok())
        .unwrap_or_else(|| quote_of(candidate))
}

fn new_attempt(
    unit: &MediaUnitRef,
    provider: &str,
    status: AttemptStatus,
    candidate: Option<Value>,
    quote: Option<Value>,
) -> AcquisitionAttempt {
    let now = jobs::now_ms();
    AcquisitionAttempt {
        id: scan::short_hash(&format!("attempt|{}|{}", unit.key(), auth::random_token())),
        unit_key: unit.key(),
        request_id: None,
        provider: provider.to_string(),
        status,
        error_code: String::new(),
        error_message: String::new(),
        candidate,
        quote,
        job_id: String::new(),
        cost_points: 0,
        unlock_key: None,
        created_at: now,
        updated_at: now,
        finished_at: None,
    }
}

/// The ledger's partial unique index owns the one-live-attempt rule; turning its
/// rejection into a sentence keeps a lost race from reading as a server bug.
fn insert_attempt(state: &SharedState, attempt: &AcquisitionAttempt) -> anyhow::Result<()> {
    db::create_attempt(&state.db, attempt).map_err(|error| {
        tracing::debug!(
            unit = %attempt.unit_key,
            error = %error,
            "an attempt for this unit is already live"
        );
        anyhow::anyhow!("an acquisition for this title is already in progress")
    })
}

/// `kroma-db`'s attempt API creates a row and moves its status; the job id,
/// cost and unlock key only become known after the provider answers, so they are
/// written here in one place rather than by a second insert.
fn record_acquire(
    pool: &db::Pool,
    attempt_id: &str,
    job_id: &str,
    cost_points: u32,
    unlock_key: Option<&str>,
    now: i64,
) -> anyhow::Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "UPDATE acquisition_attempts SET job_id = ?2, cost_points = ?3, unlock_key = ?4, \
            updated_at = ?5 WHERE id = ?1",
        rusqlite::params![attempt_id, job_id, i64::from(cost_points), unlock_key, now],
    )?;
    drop(conn);
    Ok(())
}

/// Which answer one start step ended in.
enum ProviderStep {
    Reply(AcquisitionResponse),
    Fallback(Vec<String>),
}

/// The coarse state every acquisition endpoint answers with. `state` is the
/// union a client switches on; the provider-specific fields ride beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AcquisitionState {
    Owned,
    NeedsConfirm,
    Started,
    PendingReview,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcquisitionResponse {
    state: AcquisitionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    job_id: Option<String>,
    /// The provider's own candidate JSON, exactly as `/confirm` expects it back.
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote: Option<QuoteView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owned: Option<OwnedState>,
    /// Placement work left for the module (upload, download, evict) when the
    /// unit is already held somewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<NextAction>,
    /// Why a provider was skipped (readiness, no candidates, unreachable), so a
    /// BT fallback still explains itself.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    blockers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl AcquisitionResponse {
    fn new(state: AcquisitionState) -> Self {
        Self {
            state,
            provider: None,
            request_id: None,
            attempt_id: None,
            job_id: None,
            candidate: None,
            quote: None,
            owned: None,
            next: None,
            blockers: Vec::new(),
            error: None,
        }
    }

    fn provider(mut self, provider: &str) -> Self {
        self.provider = Some(provider.to_string());
        self
    }

    fn request(mut self, request_id: &str) -> Self {
        self.request_id = Some(request_id.to_string());
        self
    }

    fn attempt(mut self, attempt_id: &str) -> Self {
        self.attempt_id = Some(attempt_id.to_string());
        self
    }

    fn job(mut self, job_id: &str) -> Self {
        self.job_id = Some(job_id.to_string());
        self
    }

    fn candidate(mut self, candidate: Value) -> Self {
        self.candidate = Some(candidate);
        self
    }

    fn quote(mut self, quote: QuoteView) -> Self {
        self.quote = Some(quote);
        self
    }

    fn owned(mut self, owned: OwnedState) -> Self {
        self.owned = Some(owned);
        self
    }

    fn next(mut self, next: NextAction) -> Self {
        self.next = Some(next);
        self
    }

    fn blockers(mut self, blockers: Vec<String>) -> Self {
        self.blockers = blockers;
        self
    }

    fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }
}

/// `GET /api/acquisition/status`: the ledger row plus where its bytes live.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusView {
    state: AcquisitionState,
    /// The ledger status verbatim (`waiting_confirm`, `transferring`, `done`…),
    /// finer than `state`.
    status: AttemptStatus,
    attempt: AcquisitionAttempt,
    locations: Vec<MediaLocation>,
    owned: OwnedState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_support::{get, seed_session, send, test_app};
    use axum::http::StatusCode;
    use kroma_domain::{LocationState, MediaUnitRef, PROVIDER_CLOUD115};

    fn a_hit(raw: Value) -> Hit {
        let candidate = normalize_candidate(&raw).expect("a valid candidate");
        Hit { candidate, raw }
    }

    fn candidate_json(id: &str, price: Option<u32>, known: bool) -> Value {
        json!({
            "id": id,
            "provider": PROVIDER_CLOUD115,
            "title": format!("release {id}"),
            "pricePoints": price,
            "priceKnown": known,
            "grabbable": true,
        })
    }

    #[test]
    fn acquisition_library_resolution_uses_video_kind_and_global_movie_preference() {
        use crate::services::settings::LibraryDef;
        use kroma_domain::AcquisitionMode;

        let state = crate::api::test_support::test_app().state;
        let pool = &state.db;
        let mut defs = vec![
            LibraryDef {
                id: "photos".into(),
                name: "Photos".into(),
                kind: "photos".into(),
                folders: vec!["/media/photos".into()],
                auto_scan: true,
            },
            LibraryDef {
                id: "films".into(),
                name: "Films".into(),
                kind: "movies".into(),
                folders: vec!["/media/movies".into()],
                auto_scan: true,
            },
        ];
        crate::services::settings::set_library_defs(&state.settings, pool, &defs);
        crate::services::settings::set_library_acquisition(
            &state.settings,
            pool,
            "films",
            AcquisitionMode::HybridLocalFirst,
        );
        state.settings.set_patch(
            pool,
            std::collections::BTreeMap::from([("acqMovieLibrary".to_string(), json!("Films"))]),
        );

        let chosen = movie_ask(&state, 1, "Film", Some(2020), None).unwrap();
        assert_eq!(chosen.library.as_deref(), Some("Films"));
        assert_eq!(chosen.library_id.as_deref(), Some("films"));
        assert_eq!(chosen.policy, AcquisitionMode::HybridLocalFirst.policy());

        defs.swap(0, 1);
        crate::services::settings::set_library_defs(&state.settings, pool, &defs);
        let invalid = movie_ask(&state, 1, "Film", Some(2020), Some("Photos"));
        assert!(
            invalid.is_err(),
            "a photo library cannot receive a movie request"
        );
    }

    #[test]
    fn a_candidate_list_reads_bare_or_wrapped() {
        let bare = hits_of(json!([candidate_json("a", Some(0), true)]));
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[0].candidate.id, "a");
        assert_eq!(bare[0].candidate.provider, PROVIDER_CLOUD115);

        let wrapped = hits_of(json!({ "candidates": [candidate_json("b", None, false)] }));
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0].candidate.id, "b");
        assert!(!wrapped[0].candidate.price_known);
    }

    #[test]
    fn a_candidate_without_an_id_is_ignored() {
        let hits = hits_of(json!([
            { "provider": PROVIDER_CLOUD115, "grabbable": true },
            candidate_json("kept", Some(0), true),
        ]));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].candidate.id, "kept");
    }

    #[test]
    fn a_candidate_that_says_nothing_about_grabbing_is_grabbable() {
        let hits = hits_of(json!([{ "id": "c", "priceKnown": true, "pricePoints": 0 }]));
        assert!(hits[0].candidate.grabbable);
        assert_eq!(hits[0].candidate.provider, PROVIDER_CLOUD115);
    }

    #[test]
    fn the_best_candidate_prefers_score_then_a_known_cheaper_price() {
        let mut low = candidate_json("low", Some(9), true);
        low["extra"] = json!({ "score": 1 });
        let mut scored = candidate_json("scored", Some(9), true);
        scored["extra"] = json!({ "score": 5 });
        let mut scored_free = candidate_json("scored-free", Some(0), true);
        scored_free["extra"] = json!({ "score": 5 });

        let hits = vec![a_hit(low), a_hit(scored), a_hit(scored_free)];
        assert_eq!(best_candidate(&hits).unwrap().candidate.id, "scored-free");

        // Same score, one price unknown: the known one wins.
        let mut unknown = candidate_json("unknown", None, false);
        unknown["extra"] = json!({ "score": 2 });
        let mut known = candidate_json("known", Some(7), true);
        known["extra"] = json!({ "score": 2 });
        let hits = vec![a_hit(unknown), a_hit(known)];
        assert_eq!(best_candidate(&hits).unwrap().candidate.id, "known");
    }

    #[test]
    fn a_rejected_or_ungrabbable_candidate_is_never_chosen() {
        let mut rejected = candidate_json("rejected", Some(0), true);
        rejected["rejectReason"] = json!("wrong edition");
        let mut stuck = candidate_json("stuck", Some(0), true);
        stuck["grabbable"] = json!(false);

        let hits = vec![a_hit(rejected), a_hit(stuck)];
        assert!(best_candidate(&hits).is_none());
    }

    #[test]
    fn only_a_known_free_candidate_skips_confirmation() {
        assert!(!needs_confirm(
            &a_hit(candidate_json("free", Some(0), true)).candidate
        ));
        assert!(needs_confirm(
            &a_hit(candidate_json("paid", Some(3), true)).candidate
        ));
        assert!(needs_confirm(
            &a_hit(candidate_json("null", None, true)).candidate
        ));
        assert!(needs_confirm(
            &a_hit(candidate_json("unknown", None, false)).candidate
        ));
    }

    #[test]
    fn an_unlocked_candidate_skips_confirmation_whatever_it_costs() {
        let mut unlocked = candidate_json("owned", Some(50), true);
        unlocked["extra"] = json!({ "unlocked": true });
        assert!(!needs_confirm(&a_hit(unlocked).candidate));
    }

    #[test]
    fn the_same_candidate_is_matched_by_provider_and_id() {
        let stored = candidate_json("one", Some(0), true);
        let mut same = candidate_json("one", Some(9), true);
        same["title"] = json!("renamed");
        let other = candidate_json("two", Some(0), true);
        assert!(same_candidate(&stored, &same));
        assert!(!same_candidate(&stored, &other));
        assert!(!same_candidate(&stored, &json!({})));
    }

    #[test]
    fn a_provider_status_maps_onto_the_ledger() {
        assert_eq!(
            status_from_point("transferring"),
            Some(AttemptStatus::Transferring)
        );
        assert_eq!(status_from_point("Done"), Some(AttemptStatus::Done));
        assert_eq!(status_from_point("success"), Some(AttemptStatus::Done));
        assert_eq!(status_from_point("failed"), Some(AttemptStatus::Failed));
        assert_eq!(
            status_from_point("cancelled"),
            Some(AttemptStatus::Cancelled)
        );
        assert_eq!(status_from_point("something new"), None);
    }

    #[test]
    fn an_acquire_reply_reads_a_job_id_from_a_string_or_an_object() {
        assert_eq!(AcquireReply::parse(json!("job-1")).unwrap().job_id, "job-1");
        let object = AcquireReply::parse(json!({ "jobId": "job-2", "points": 0 })).unwrap();
        assert_eq!(object.job_id, "job-2");
        assert_eq!(object.points, Some(0));
        assert!(AcquireReply::parse(json!({ "jobId": "" })).is_err());
        assert!(AcquireReply::parse(json!(42)).is_err());
    }

    #[test]
    fn a_needs_confirm_reply_is_camel_case_and_carries_the_quote() {
        let reply = AcquisitionResponse::new(AcquisitionState::NeedsConfirm)
            .provider(PROVIDER_CLOUD115)
            .attempt("attempt-1")
            .candidate(candidate_json("one", Some(3), true))
            .quote(QuoteView {
                points: Some(3),
                price_known: true,
            });
        let value = serde_json::to_value(&reply).expect("serialize");
        assert_eq!(value["state"], "needs_confirm");
        assert_eq!(value["provider"], "cloud115");
        assert_eq!(value["attemptId"], "attempt-1");
        assert_eq!(value["quote"], json!({ "points": 3, "priceKnown": true }));
        assert_eq!(value["candidate"]["id"], "one");
        assert!(value.get("jobId").is_none());
        assert!(value.get("requestId").is_none());
        assert!(value.get("error").is_none());
    }

    #[test]
    fn an_owned_reply_names_what_the_library_holds_and_the_next_step() {
        let reply = owned_reply(
            OwnedState {
                local: false,
                cloud: true,
            },
            NextAction::Complete,
        );
        let value = serde_json::to_value(&reply).expect("serialize");
        assert_eq!(value["state"], "owned");
        assert_eq!(value["owned"], json!({ "local": false, "cloud": true }));
        assert_eq!(value["next"], "complete");
    }

    #[test]
    fn the_router_builds() {
        let _app = routes();
    }

    #[test]
    fn an_approved_show_request_names_the_bt_provider_for_the_client_contract() {
        let value = serde_json::to_value(show_reply(RequestStatus::Approved, "request-1", None))
            .expect("serialize");

        assert_eq!(value["state"], "started");
        assert_eq!(value["provider"], PROVIDER_BT);
        assert_eq!(value["requestId"], "request-1");
    }

    #[test]
    fn a_pending_show_request_is_a_review_reply_not_a_provider_start() {
        let value = serde_json::to_value(show_reply(RequestStatus::Pending, "request-2", None))
            .expect("serialize");

        assert_eq!(value["state"], "pending_review");
        assert_eq!(value["requestId"], "request-2");
        assert!(value.get("provider").is_none());
    }

    #[test]
    fn a_closed_show_request_reply_has_an_error_for_the_client_contract() {
        let value = serde_json::to_value(show_reply(
            RequestStatus::Denied,
            "request-3",
            Some("not approved"),
        ))
        .expect("serialize");

        assert_eq!(value["state"], "failed");
        assert_eq!(value["error"], "not approved");
    }

    #[tokio::test]
    async fn a_film_the_cloud_already_holds_is_owned_without_calling_a_provider() {
        let app = test_app();
        crate::services::settings::set_library_defs(
            &app.state.settings,
            &app.state.db,
            &[crate::services::settings::LibraryDef {
                id: "films".into(),
                name: "Films".into(),
                kind: "movies".into(),
                folders: vec!["/media/movies".into()],
                auto_scan: true,
            }],
        );
        let unit = MediaUnitRef::movie(987_654);
        {
            let conn = app.state.db.get().expect("db conn");
            crate::db::upsert_media_unit(&conn, &unit, "Already There", Some(2024), 1)
                .expect("media unit");
            crate::db::upsert_media_location(
                &conn,
                &MediaLocation {
                    id: "loc-1".into(),
                    unit_key: unit.key(),
                    provider: PROVIDER_CLOUD115.into(),
                    account_id: "account".into(),
                    path: "/电影/Already There.mkv".into(),
                    remote_id: "remote-1".into(),
                    origin: "imported".into(),
                    state: LocationState::Present,
                    is_primary: false,
                    size: None,
                    fingerprint: None,
                    quality: None,
                    verified_at: Some(1),
                    created_at: 1,
                    updated_at: 1,
                },
            )
            .expect("cloud location");
        }

        let (status, body) = send(
            &app.app,
            "POST",
            "/api/acquisition/start",
            Some(&app.token),
            Some(json!({
                "kind": "movie",
                "tmdbId": 987_654,
                "title": "Already There",
                "year": 2024,
            })),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["state"], "owned");
        assert_eq!(body["owned"], json!({ "local": false, "cloud": true }));
        assert_eq!(body["next"], "complete");
    }

    #[tokio::test]
    async fn starting_a_film_needs_the_requests_create_capability() {
        let app = test_app();
        let (_id, viewer) = seed_session(
            &app.state,
            "viewer@test.dev",
            "viewer",
            &[Permission::Playback],
        );

        let (status, _) = send(
            &app.app,
            "POST",
            "/api/acquisition/start",
            Some(&viewer),
            Some(json!({ "kind": "movie", "tmdbId": 1, "title": "Film" })),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn an_unknown_attempt_is_not_found() {
        let app = test_app();

        let (status, body) = get(
            &app.app,
            "/api/acquisition/status?attemptId=nope",
            Some(&app.token),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["code"], "error.itemNotFound");
    }

    #[tokio::test]
    async fn a_title_without_a_tmdb_id_is_refused() {
        let app = test_app();

        let (status, _) = send(
            &app.app,
            "POST",
            "/api/acquisition/start",
            Some(&app.token),
            Some(json!({ "kind": "movie", "tmdbId": 0, "title": "Film" })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// A show's candidate now takes the same transfer path its `/start` does. With
/// no cloud provider installed, that path stops at "the provider is missing"
/// (404), which is a different answer from the old flat "a show is not
/// confirmed here" (400) — proof the request is no longer refused outright.
#[tokio::test]
    async fn confirming_a_show_reaches_the_show_path_instead_of_being_refused() {
        let app = test_app();

        let (status, _) = send(
            &app.app,
            "POST",
            "/api/acquisition/confirm",
            Some(&app.token),
            Some(json!({
                "kind": "show",
                "tmdbId": 1396,
                "title": "Show",
                "season": 1,
                "candidate": candidate_json("one", Some(0), true),
            })),
        )
        .await;

        assert_ne!(status, StatusCode::BAD_REQUEST);
    }
}
