#![cfg(feature = "contract")]
use veoveo_mcp_embedding::contract::*;
#[test]
fn input_contract_rejects_overflow_unknown_purpose_and_overrides() {
    let query = || EmbeddingInput::SearchQuery {
        text: "synthetic query".into(),
    };
    assert!(
        EmbedRequest {
            items: vec![query()]
        }
        .validate()
        .is_ok()
    );
    assert!(EmbedRequest { items: vec![] }.validate().is_err());
    assert!(
        EmbedRequest {
            items: vec![query(); 9]
        }
        .validate()
        .is_err()
    );
    assert!(
        EmbedRequest {
            items: vec![EmbeddingInput::Document {
                content: "a".repeat(MAX_CONTENT_BYTES),
                title: Some("title".into())
            }]
        }
        .validate()
        .is_err()
    );
    assert!(
        EmbedRequest {
            items: vec![EmbeddingInput::SearchQuery { text: " ".into() }]
        }
        .validate()
        .is_err()
    );
    for value in [
        r#"{"items":[],"model":"other"}"#,
        r#"{"items":[{"purpose":"clustering","text":"a"}]}"#,
        r#"{"items":[{"purpose":"search_query","text":"a","title":"invalid"}]}"#,
    ] {
        assert!(serde_json::from_str::<EmbedRequest>(value).is_err());
    }
}
#[test]
fn addresses_use_native_resource_profiles() {
    use veoveo_types::ResourceAddress;
    let resource = EmbeddingResource::parse("embedding://capabilities").unwrap();
    assert_eq!(
        resource.to_uri().unwrap().as_str(),
        "embedding://capabilities"
    );
    assert!(EmbeddingResource::parse("embedding://capabilities?model=other").is_err());
}

#[test]
fn capabilities_reader_is_a_public_typed_resource_contract() {
    let _reader = ReadCapabilities;
}
