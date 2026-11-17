use kroma_module_runtime::RemoteHost;

kroma_module_runtime::module_main!(
    |_host: &RemoteHost| axum::Router::new(),
    vec![kroma_subscriptions::server_module::<RemoteHost>()],
);
