#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use axum::routing::get;
    use tower_http::services::ServeDir;

    streamview::server_fns::register_all();

    // In dev, trunk serve handles static files and proxies /api/* here. For
    // a standalone run (`trunk build --release` once, then just this
    // binary), this same process also serves the built dist/ directory --
    // ServeDir serves dist/index.html for "/" and falls back to it for any
    // other unmatched path too, in case client-side routing is ever added.
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "dist".to_string());
    let index_html = format!("{static_dir}/index.html");

    let app: Router = Router::new()
        .route(
            "/api/{*fn_name}",
            get(leptos_axum::handle_server_fns).post(leptos_axum::handle_server_fns),
        )
        .fallback_service(ServeDir::new(&static_dir).fallback(axum::routing::get_service(
            tower_http::services::ServeFile::new(&index_html),
        )));

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {bind_addr}: {e}"));
    println!("streamview listening on http://{bind_addr} (static files from {static_dir}/)");
    axum::serve(listener, app).await.expect("server error");
}

#[cfg(not(feature = "ssr"))]
fn main() {
    leptos::mount::mount_to_body(streamview::App);
}
