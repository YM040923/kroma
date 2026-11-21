//! Household scraping defaults: the language list and the artwork rules every
//! library follows until it sets its own. Persisted under `scrapeDefaults`,
//! with the primary language mirrored into `tmdbLanguage` so the enrichment
//! engine reads it unchanged.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use serde_json::{json, Value};

use crate::api::error::lerr;
use crate::api::extract::AuthUser;
use crate::model::Permission;
use crate::services::settings::{self, ScrapeDefaults};
use crate::state::SharedState;

pub fn routes() -> Router<SharedState> {
    Router::new().route("/scrape", get(get_scrape).patch(put_scrape))
}

fn view(state: &SharedState, defaults: &ScrapeDefaults) -> Value {
    let mut value = serde_json::to_value(defaults).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert("languages".to_string(), json!(settings::language_tags()));
        object.insert(
            "fanartConfigured".to_string(),
            json!(!settings::fanart_api_key(&state.settings).is_empty()),
        );
    }
    value
}

async fn get_scrape(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    let defaults = settings::scrape_defaults(&state.settings, &state.config);
    Ok(Json(view(&state, &defaults)).into_response())
}

async fn put_scrape(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Json(body): Json<Value>,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    let object = body.as_object().cloned().unwrap_or_default();
    let clear_key = object
        .get("clearFanartKey")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let fanart_key = object
        .get("fanartApiKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string);
    if let Some(key) = fanart_key.clone() {
        let checked =
            tokio::task::spawn_blocking(move || crate::infra::metadata::fanart::verify_key(&key))
                .await;
        match checked {
            Ok(Ok(())) => {}
            Ok(Err(crate::infra::metadata::fanart::VerifyError::InvalidKey)) => {
                return Err(lerr(
                    super::user_locale(&user),
                    StatusCode::BAD_REQUEST,
                    "admin.fanartKeyInvalid",
                ))
            }
            Ok(Err(crate::infra::metadata::fanart::VerifyError::Unreachable)) | Err(_) => {
                return Err(lerr(
                    super::user_locale(&user),
                    StatusCode::BAD_GATEWAY,
                    "admin.fanartKeyUnreachable",
                ))
            }
        }
    }
    let has_rules = object
        .keys()
        .any(|key| key != "fanartApiKey" && key != "clearFanartKey");
    if has_rules {
        // Merge, not replace: a patch may carry one rule (or a key-only body
        // after the UI's other fields were defaulted away), and the saved
        // object must keep everything it did not mention.
        let current = settings::scrape_defaults(&state.settings, &state.config);
        let mut merged = serde_json::to_value(&current).unwrap_or_else(|_| json!({}));
        if let Some(target) = merged.as_object_mut() {
            for (key, value) in &object {
                if key != "fanartApiKey" && key != "clearFanartKey" {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        let mut stored: ScrapeDefaults = serde_json::from_value(merged).map_err(|_| {
            lerr(
                super::user_locale(&user),
                StatusCode::BAD_REQUEST,
                "admin.scrapeInvalid",
            )
        })?;
        if object.contains_key("languagePriority") {
            let Some(priority) = stored.language_priority.clone() else {
                return Err(lerr(
                    super::user_locale(&user),
                    StatusCode::BAD_REQUEST,
                    "admin.scrapeLanguageInvalid",
                ));
            };
            let known = settings::language_tags();
            let priority = settings::clean_language_priority(priority);
            if priority.is_empty()
                || priority
                    .iter()
                    .any(|tag| !known.iter().any(|option| option.eq_ignore_ascii_case(tag)))
            {
                return Err(lerr(
                    super::user_locale(&user),
                    StatusCode::BAD_REQUEST,
                    "admin.scrapeLanguageInvalid",
                ));
            }
            stored.language_priority = Some(priority);
        }
        settings::set_scrape_defaults(&state.settings, &state.db, &stored);
    }
    if clear_key {
        settings::set_fanart_api_key(&state.settings, &state.db, "");
    } else if let Some(key) = fanart_key.as_deref() {
        settings::set_fanart_api_key(&state.settings, &state.db, key);
    }
    let saved = settings::scrape_defaults(&state.settings, &state.config);
    Ok(Json(view(&state, &saved)).into_response())
}
