//! Household scraping defaults: the language list every library follows until
//! it sets its own. Persisted under `scrapeDefaults`, with the primary tag
//! mirrored into `tmdbLanguage` so the enrichment engine reads it unchanged.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use serde::Deserialize;
use serde_json::json;

use crate::api::error::lerr;
use crate::api::extract::AuthUser;
use crate::model::Permission;
use crate::services::settings;
use crate::state::SharedState;

pub fn routes() -> Router<SharedState> {
    Router::new().route("/scrape", get(get_scrape).patch(put_scrape))
}

async fn get_scrape(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    let defaults = settings::scrape_defaults(&state.settings, &state.config);
    Ok(Json(json!({
        "languagePriority": defaults.language_priority.unwrap_or_default(),
        "languages": settings::language_tags(),
    }))
    .into_response())
}

#[derive(Debug, Deserialize)]
pub struct ScrapeBody {
    #[serde(rename = "languagePriority", default)]
    pub language_priority: Vec<String>,
}

async fn put_scrape(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Json(body): Json<ScrapeBody>,
) -> Result<Response, Response> {
    super::require(&user, Permission::SettingsManage)?;
    let priority = settings::clean_language_priority(body.language_priority);
    let known = settings::language_tags();
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
    settings::set_scrape_defaults(&state.settings, &state.db, priority.clone());
    Ok(Json(json!({ "languagePriority": priority })).into_response())
}