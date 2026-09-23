pub mod app;
pub mod components;
pub mod server_fns;
pub mod types;

// datafusion/headwater aren't wasm-compatible (and aren't even pulled in as
// dependencies under the csr feature), so this module only exists in the
// ssr build. The `#[server]` function bodies that reference it are
// themselves cfg-gated by the `#[server]` macro.
#[cfg(feature = "ssr")]
pub mod query;

pub use app::App;
