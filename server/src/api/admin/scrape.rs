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

fn view(defaults: &ScrapeDefaults) -> Value {
    let mut value = serde_json::to_value(defaults).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert("languages".to_string(), json!(settings::language_tags()));
    }
    value
}

async fn get_scrape(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    let defaults = settings::scrape_defaults(&state.settings, &state.config);
    Ok(Json(view(&defaults)).into_response())
}

async fn put_scrape(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Json(body): Json<ScrapeDefaults>,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    let priority = body.language_priority.clone().unwrap_or_default();
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
    let mut stored = body;
    stored.language_priority = Some(priority);
    settings::set_scrape_defaults(&state.settings, &state.db, &stored);
    let saved = settings::scrape_defaults(&state.settings, &state.config);
    Ok(Json(view(&saved)).into_response())
}