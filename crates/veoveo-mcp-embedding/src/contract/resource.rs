//! The server's resource addresses under the `embedding` scheme. The route declaration
//! owns each shape and its typed parameters; the shared URI machinery owns encoding and
//! parsing, so no address is built or split by hand.
use std::{borrow::Cow, error::Error, fmt};

use veoveo_types::{ResourceFieldCodec, ResourceRouteError, ResourceUriError, Vocabulary};

/// A resource address that failed admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingResourceError {
    Uri(ResourceUriError),
    UnknownResource,
}

impl fmt::Display for EmbeddingResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uri(error) => error.fmt(f),
            Self::UnknownResource => {
                f.write_str("unknown or noncanonical embedding resource address")
            }
        }
    }
}

impl Error for EmbeddingResourceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Uri(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ResourceUriError> for EmbeddingResourceError {
    fn from(error: ResourceUriError) -> Self {
        Self::Uri(error)
    }
}

/// A document the server embeds and serves under `embedding://docs/{doc_id}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Vocabulary)]
pub enum EmbeddingDocument {
    /// The agent work manual, `AGENTS.md`.
    Agents,
    /// The design, `DESIGN.md`.
    Design,
}

/// A resource of the server. A usable address carries only the identities and cursor of
/// its own family.
#[veoveo_types::resource_address(routes(EmbeddingAddresses), schema = derived, wire, parse = borrowed, display)]
pub enum EmbeddingResource {
    #[resource(template = "embedding://docs")]
    Docs,
    #[resource(template = "embedding://docs/{doc_id}")]
    Document(#[resource(variable = "doc_id", codec = EmbeddingFieldCodec)] EmbeddingDocument),
    #[resource(template = "embedding://contract")]
    Contract,
    #[resource(template = "embedding://capabilities")]
    Capabilities,
}

fn route_error(error: ResourceRouteError) -> EmbeddingResourceError {
    match error {
        ResourceRouteError::Uri(error) => EmbeddingResourceError::Uri(error),
        _ => EmbeddingResourceError::UnknownResource,
    }
}

struct EmbeddingFieldCodec;

impl ResourceFieldCodec<EmbeddingDocument> for EmbeddingFieldCodec {
    type Error = EmbeddingResourceError;
    fn parse(value: &str) -> Result<EmbeddingDocument, Self::Error> {
        value
            .parse()
            .map_err(|_| EmbeddingResourceError::UnknownResource)
    }
    fn text(value: &EmbeddingDocument) -> Cow<'_, str> {
        value.as_str().into()
    }
}

#[doc(hidden)]
pub struct EmbeddingAddresses;
impl veoveo_types::ResourceProfile for EmbeddingAddresses {
    type Error = EmbeddingResourceError;
    const PROFILE: veoveo_types::ResourceProfileSpec<Self::Error> =
        veoveo_types::ResourceProfileSpec {
            route_error: |_, error| route_error(error),
        };
}

/// Read the startup-validated capability snapshot. Readiness is checked separately.
#[derive(Debug, Clone)]
#[veoveo_hub_sdk::resource_read(provider = super::EmbeddingProvider, address_type = EmbeddingResource,
    content = super::Capabilities, template = "embedding://capabilities", address = EmbeddingResource::Capabilities,
    parse = |address| matches!(address, EmbeddingResource::Capabilities).then_some(Self))]
pub struct ReadCapabilities;
