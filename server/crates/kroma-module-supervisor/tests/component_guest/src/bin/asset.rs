use axum::extract::{Path, RawQuery};
use axum::routing::get;
use axum::Router;
use kroma_module_runtime::RemoteHost;

async fn download(Path(port): Path<u16>, RawQuery(mode): RawQuery) -> String {
    let destination = match mode.as_deref() {
        Some("other") => "/selected/not-granted/Demo.nfo",
        _ => "/selected/films/有 空格/电影 (2026).nfo",
    };
    let url = format!("http://127.0.0.1:{port}/fixture.nfo");
    match kroma_module_sdk::http::Fetch::new()
        .purpose("probe")
        .max_time(8)
        .download(&url, std::path::Path::new(destination))
    {
        Ok(response) => response.status.to_string(),
        Err(error) => format!("refused: {error}"),
    }
}

fn routes(_: &RemoteHost) -> Router<RemoteHost> {
    Router::new().route("/fixture-download/{port}", get(download))
}

kroma_module_runtime::module_main!(routes, vec![]);
