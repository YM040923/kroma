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
use serde::Deserialize;
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

#[derive(Debug, Deserialize)]
pub struct ScrapeBody {
    #[serde(flatten)]
    pub defaults: ScrapeDefaults,
    #[serde(rename = "fanartApiKey", default)]
    pub fanart_api_key: Option<String>,
    #[serde(rename = "clearFanartKey", default)]
    pub clear_fanart_key: bool,
}

async fn put_scrape(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Json(body): Json<ScrapeBody>,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    // A key-only patch must not wipe the saved rules.
    if let Some(priority) = body.defaults.language_priority.clone() {
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
        let mut stored = body.defaults.clone();
        stored.language_priority = Some(priority);
        settings::set_scrape_defaults(&state.settings, &state.db, &stored);
    }
    if body.clear_fanart_key {
        settings::set_fanart_api_key(&state.settings, &state.db, "");
    } else if let Some(key) = body.fanart_api_key.as_deref().map(str::trim) {
        if !key.is_empty() {
            settings::set_fanart_api_key(&state.settings, &state.db, key);
        }
    }
    let saved = settings::scrape_defaults(&state.settings, &state.config);
    Ok(Json(view(&state, &saved)).into_response())
}