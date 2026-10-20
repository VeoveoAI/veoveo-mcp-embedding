#![cfg(feature = "contract")]
use serde_json::{Value, json};
use veoveo_mcp_embedding::contract::*;
fn request(options: Value) -> Result<EmbedRequest, serde_json::Error> {
    let mut value = json!({"items":[{"content":[{"type":"text","text":"hello again"}]}]});
    value
        .as_object_mut()
        .unwrap()
        .extend(options.as_object().unwrap().clone());
    serde_json::from_value(value)
}
#[test]
fn ordered_parts_dimensions_and_optional_native_prompt_are_closed() {
    let base = request(json!({})).unwrap();
    assert_eq!(base.effective_prompt(), "");
    assert_eq!(base.dimensions, Dimensions::D768);
    assert!(base.validate().is_ok());
    let item = json!({"content":[{"type":"text","text":"before"},{"type":"image","artifactId":"019a0000-0000-7000-8000-000000000001"},{"type":"text","text":"after"}]});
    let mixed =
        request(json!({"items":[item],"dimensions":256,"promptName":"SearchQuery"})).unwrap();
    assert_eq!(
        mixed.items[0]
            .content
            .iter()
            .map(ContentPart::modality)
            .collect::<Vec<_>>(),
        [Modality::Text, Modality::Image, Modality::Text]
    );
    assert_eq!(mixed.effective_prompt(), PromptName::SearchQuery.prefix());
    assert_eq!(mixed.dimensions, Dimensions::D256);
    assert!(mixed.validate().is_ok());
    for dimension in [128, 256, 512, 768] {
        assert!(
            request(json!({"dimensions":dimension}))
                .unwrap()
                .validate()
                .is_ok()
        );
    }
    for value in [
        json!({"dimensions":129}),
        json!({"dimensions":true}),
        json!({"prompt":null}),
        json!({"promptName":"query"}),
        json!({"promptName":null}),
        json!({"model":"other"}),
        json!({"items":[{"purpose":"search_query","text":"a"}]}),
        json!({"items":[{"content":[{"type":"image","url":"https://example.test/x"}]}]}),
        json!({"preprocessing":{"image":{"max_soft_tokens":70}}}),
        json!({"preprocessing":{"audio":{"maxLength":null}}}),
    ] {
        assert!(request(value.clone()).is_err(), "{value}");
    }
    assert!(
        request(json!({"prompt":"","promptName":"Document"}))
            .unwrap()
            .validate()
            .is_err()
    );
}
#[test]
fn byte_part_batch_and_preprocessing_bounds_match_runtime() {
    for value in [
        json!({"items":[]}),
        json!({"items":[{"content":[]}]}),
        json!({"items":vec![json!({"content":[{"type":"text","text":"a"}]} );9]}),
        json!({"items":[{"content":vec![json!({"type":"text","text":"a"});17]}]}),
        json!({"items":[{"content":[{"type":"text","text":"a".repeat(MAX_CONTENT_BYTES)}]}],"prompt":"é"}),
        json!({"items":[{"content":[{"type":"text","text":" "}]}]}),
        json!({"prompt":"<|video|>"}),
        json!({"preprocessing":{"image":{"maxSoftTokens":71}}}),
        json!({"preprocessing":{"audio":{"maxLength":1}}}),
        json!({"preprocessing":{"video":{"fps":0}}}),
        json!({"preprocessing":{"video":{"fps":121}}}),
        json!({"preprocessing":{"video":{"maxFrames":257}}}),
        json!({"items":[{"content":[{"type":"audio","artifactId":"019a0000-0000-7000-8000-000000000001"}]}],"preprocessing":{"video":{"loadAudioFromVideo":true}}}),
    ] {
        assert!(
            request(value.clone()).unwrap().validate().is_err(),
            "{value}"
        );
    }
    for tokens in SOFT_TOKENS {
        assert!(request(json!({"preprocessing":{"image":{"maxSoftTokens":tokens},"video":{"maxSoftTokens":tokens}}})).unwrap().validate().is_ok());
    }
    let audio = request(json!({"preprocessing":{"audio":{"truncation":true}}})).unwrap();
    assert_eq!(
        audio.preprocessing.effective().audio.max_length,
        Some(480000)
    );
    assert_eq!(Preprocessing::default().audio.max_length, None);
    assert!(!Preprocessing::default().audio.truncation);
}
#[test]
fn public_schema_contains_artifact_ids_and_never_private_transfers() {
    let schema = serde_json::to_value(schemars::schema_for!(EmbedRequest))
        .unwrap()
        .to_string();
    assert!(schema.contains("artifactId"));
    for private in [
        "SignedTransfer",
        "RuntimeSource",
        "RuntimeRequest",
        "expiresAt",
        "\"url\"",
        "\"get\"",
    ] {
        assert!(!schema.contains(private), "{private}");
    }
    let dimensions = serde_json::to_value(schemars::schema_for!(Dimensions)).unwrap();
    assert_eq!(dimensions["enum"], json!([128, 256, 512, 768]));
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
    let _reader = ReadCapabilities;
}
