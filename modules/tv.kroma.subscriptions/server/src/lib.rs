#![allow(clippy::result_large_err)]

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
    // Discovery is intentionally dry-run in the first slice. No 115 write or
    // paid acquisition happens until an item is explicitly executed.
    let _ = store::load();
    routes::poll_transfers();
    routes::discover();
    Ok(())
}

pub fn server_module<S: HostCtx + Clone + Send + Sync + 'static>() -> Box<dyn ServerModule<S>> {
    Box::new(SubscriptionsModule)
}
