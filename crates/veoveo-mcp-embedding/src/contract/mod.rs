//! Public contract only: no network, database, Tasks or model runtime.
mod permissions;
mod resource;
mod types;
pub use permissions::*;
pub use resource::*;
pub use types::*;
use veoveo_hub_sdk::calls::ProviderContract;
pub struct EmbeddingProvider;
impl ProviderContract for EmbeddingProvider {
    fn slug() -> veoveo_types::ServerSlug {
        crate::server()
    }
}
