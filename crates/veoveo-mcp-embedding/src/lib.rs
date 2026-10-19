//! Bounded embedding generation through a private, authenticated GPU runtime.
#[cfg(feature = "contract")]
pub mod contract;
#[cfg(feature = "server")]
pub mod mcp;
#[cfg(feature = "server")]
pub mod provider;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod usage;

pub fn component() -> &'static str {
    env!("CARGO_PKG_NAME")
}
#[cfg(feature = "contract")]
pub fn server() -> veoveo_types::ServerSlug {
    veoveo_types::ServerSlug::parse("embedding").expect("embedding slug")
}
#[cfg(feature = "server")]
pub fn migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!()
}
