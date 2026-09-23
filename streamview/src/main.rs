#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use axum::routing::get;

    streamview::server_fns::register_all();

    let app: Router = Router::new().route(
        "/api/{*fn_name}",
        get(leptos_axum::handle_server_fns).post(leptos_axum::handle_server_fns),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind 127.0.0.1:3000");
    println!("streamview backend listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.expect("server error");
}

#[cfg(not(feature = "ssr"))]
fn main() {
    leptos::mount::mount_to_body(streamview::App);
}
