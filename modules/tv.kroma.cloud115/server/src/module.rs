//! The `tv.kroma.cloud115` module's backend behavior: its admin page and the
//! scheduled mirror.

use kroma_module_sdk::host::{async_trait, HostCtx, ModuleJob, ServerModule};

pub const MODULE_ID: &str = "tv.kroma.cloud115";

pub struct Cloud115Module;

#[async_trait]
impl<S: HostCtx + Clone + Send + Sync + 'static> ServerModule<S> for Cloud115Module {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn admin_routes(&self, _host: &S) -> Option<axum::Router<S>> {
        Some(crate::routes::routes::<S>())
    }

    fn jobs(&self) -> Vec<ModuleJob<S>> {
        vec![
            ModuleJob {
                key: "cloud115.watch",
                category: "library",
                // The poller itself enforces the configured interval; this tick
                // only bounds how late a change can be noticed.
                schedule: Some("* * * * *"),
                run: run_watch::<S>,
            },
            ModuleJob {
                key: "cloud115.sync",
                category: "library",
                // A ten-minute tick that consults `autoSyncHours`: the interval is
                // an operator setting, and a cron expression cannot read it.
                schedule: Some("*/10 * * * *"),
                run: run_sync::<S>,
            },
        ]
    }
}

fn run_watch<S: HostCtx + Clone + Send + Sync + 'static>(host: &S) -> anyhow::Result<()> {
    crate::watch::start(host.clone());
    Ok(())
}

fn run_sync<S: HostCtx + Clone + Send + Sync + 'static>(host: &S) -> anyhow::Result<()> {
    // A scheduled run that lands while a manual one is working is not a
    // failure: the manual run is already doing exactly what this tick wanted.
    if crate::sync::is_running() {
        return Ok(());
    }
    let config = crate::settings::Config::load();
    let hours = config.options.auto_sync_hours;
    if hours == 0 {
        return Ok(());
    }
    let now = crate::events::unix_now();
    let last = crate::store::load().last_auto_sync_at;
    if last > 0 && now.saturating_sub(last) < i64::from(hours) * 3600 {
        return Ok(());
    }
    if crate::budget::background_blocked(config.options.daily_call_budget) {
        return Ok(());
    }
    match crate::sync::start(host.clone(), None, false) {
        Ok(()) => {
            let _ = crate::store::patch(|stored| stored.last_auto_sync_at = now);
            Ok(())
        }
        Err(reason) if reason.contains("已经在运行") => Ok(()),
        Err(reason) => anyhow::bail!("{reason}"),
    }
}

/// This module's backend behavior, for the host's module roster.
pub fn server_module<S: HostCtx + Clone + Send + Sync + 'static>() -> Box<dyn ServerModule<S>> {
    Box::new(Cloud115Module)
}
