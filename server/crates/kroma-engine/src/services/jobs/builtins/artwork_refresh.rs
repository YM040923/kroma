//! `artwork.refresh` re-reads a show's provider artwork on the interval its
//! library asks for, so a long-running series that gains a new poster or
//! backdrop is picked up without a manual re-enrich. Manual artwork overrides
//! remain locked; movies are handled by the normal enrichment pass.

use super::prelude::*;

pub(super) const SPEC: Builtin = Builtin {
    key: JobKey("artwork.refresh"),
    category: Category::Library,
    // Early morning, before the scan-heavy jobs and after the nightly metadata pass.
    schedule: Some("15 5 * * *"),
    triggers: &[],
    run,
};

pub(super) fn run(ctx: &JobContext) -> Result<()> {
    let settings = ctx.state.settings.clone();
    let state = &ctx.state;
    if state.config.tmdb_api_key.is_none() {
        ctx.warn("no TMDB API key configured nothing to refresh");
        return Ok(());
    }
    let configured = crate::services::settings::scrape_defaults(&settings, &state.config)
        .artwork_refresh_days
        .unwrap_or(0)
        > 0;
    if !configured {
        ctx.info("scheduled artwork refresh is off");
        return Ok(());
    }
    let (checked, refreshed) = crate::services::enrich::refresh_artwork(
        state,
        |done, total| ctx.progress(done, total),
        || ctx.cancelled(),
    )?;
    ctx.info(format!(
        "checked {checked} shows due for artwork refresh, refreshed {refreshed}"
    ));
    Ok(())
}
