#![allow(clippy::result_large_err)]

pub mod follow;
pub mod routes;
pub mod store;

use kroma_module_sdk::host::{async_trait, HostCtx, ModuleJob, ServerModule};

pub const MODULE_ID: &str = "tv.kroma.subscriptions";

pub struct SubscriptionsModule;

#[async_trait]
impl<S: HostCtx + Clone + Send + Sync + 'static> ServerModule<S> for SubscriptionsModule {
    fn id(&self) -> &'static str { MODULE_ID }

    fn admin_routes(&self, _host: &S) -> Option<axum::Router<S>> {
        Some(routes::routes::<S>())
    }

    fn jobs(&self) -> Vec<ModuleJob<S>> {
        vec![ModuleJob {
            key: "subscriptions.poll",
            category: "acquisition",
            schedule: Some("*/15 * * * *"),
            run: poll::<S>,
        }]
    }
}

fn poll<S: HostCtx + Clone + Send + Sync + 'static>(_host: &S) -> anyhow::Result<()> {
    // The tick is one minute; each pass decides its own cadence. Followed shares
    // are cheap (a listing), new-release search probes prices and is paced
    // separately. Neither spends points: an approved title is the only one that
    // may, and that decision was the operator's.
    let now = kroma_module_sdk::primitives::now_ms();
    let mut stored = store::load();
    let source = stored.source_url.clone();
    routes::poll_transfers();
    if !source.trim().is_empty() {
        follow::discover(&source, &mut stored, now);
        let _ = store::save(&stored);
    }
    follow::follow_shares(now);
    Ok(())
}

pub fn server_module<S: HostCtx + Clone + Send + Sync + 'static>() -> Box<dyn ServerModule<S>> {
    Box::new(SubscriptionsModule)
}
