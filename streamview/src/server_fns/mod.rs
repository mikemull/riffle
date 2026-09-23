pub mod publish;
pub mod query;

pub use publish::{list_published_versions, publish_dataset, verify_published_version};
pub use query::{list_sites, query_readings};

#[cfg(feature = "ssr")]
fn lake_root() -> String {
    std::env::var("LAKE_ROOT").unwrap_or_else(|_| "../headwater/maumee_lake".to_string())
}

#[cfg(feature = "ssr")]
fn publish_root() -> String {
    std::env::var("PUBLISH_ROOT").unwrap_or_else(|_| "./published".to_string())
}

/// Register every `#[server]` function with the ssr backend's server-fn
/// dispatch table.
///
/// This should be unnecessary -- server functions normally self-register at
/// startup via the `inventory` crate -- but that mechanism silently finds
/// nothing in this project (verified: `server_fn::axum::server_fn_paths()`
/// returns empty at runtime), most likely because this crate's `[lib]` is
/// `crate-type = ["cdylib", "rlib"]` (required for wasm-bindgen/trunk) and
/// `inventory`'s ctor-based collection doesn't reliably run for a cdylib
/// linked into a separate native binary. Explicit registration is the
/// library's own documented escape hatch for exactly this case. Every new
/// `#[server]` function needs a line added here.
#[cfg(feature = "ssr")]
pub fn register_all() {
    leptos::server_fn::axum::register_explicit::<query::ListSites>();
    leptos::server_fn::axum::register_explicit::<query::QueryReadings>();
    leptos::server_fn::axum::register_explicit::<publish::PublishDataset>();
    leptos::server_fn::axum::register_explicit::<publish::ListPublishedVersions>();
    leptos::server_fn::axum::register_explicit::<publish::VerifyPublishedVersion>();
}
