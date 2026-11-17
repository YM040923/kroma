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
                // Four passes a day. A mirror is a listing walk plus the handful of
                // writes it needs; running it more often would spend the account's
                // call allowance on folders that did not change. The page has a
                // button for "now".
                schedule: Some("20 */6 * * *"),
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
    match crate::sync::start(host.clone(), None, false) {
        Ok(()) => Ok(()),
        Err(reason) if reason.contains("已经在运行") => Ok(()),
        Err(reason) => anyhow::bail!("{reason}"),
    }
}

/// This module's backend behavior, for the host's module roster.
pub fn server_module<S: HostCtx + Clone + Send + Sync + 'static>() -> Box<dyn ServerModule<S>> {
    Box::new(Cloud115Module)
}
