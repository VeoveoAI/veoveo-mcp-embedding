use veoveo_hub_sdk::registration::{Access, PermissionDefinition, Risk};
use veoveo_types::{PermissionName, Vocabulary};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Vocabulary)]
#[vocabulary(scope)]
pub enum EmbeddingScope {
    #[vocabulary(rename = "embedding")]
    Embedding,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Vocabulary)]
pub enum EmbeddingPermission {
    #[vocabulary(rename = "embedding.models.read")]
    ModelsRead,
    #[vocabulary(rename = "embedding.embeddings.create")]
    EmbeddingsCreate,
}
impl EmbeddingPermission {
    pub fn name(self) -> PermissionName {
        PermissionName::parse(self.as_str()).expect("embedding permission")
    }
}
impl PermissionDefinition for EmbeddingPermission {
    fn resource(self) -> &'static str {
        match self {
            Self::ModelsRead => "embedding.model",
            Self::EmbeddingsCreate => "embedding.embedding",
        }
    }
    fn access(self) -> Access {
        match self {
            Self::ModelsRead => Access::Read,
            Self::EmbeddingsCreate => Access::Write,
        }
    }
    fn risks(self) -> &'static [Risk] {
        &[]
    }
}
