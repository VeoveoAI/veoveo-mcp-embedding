//! The public contract, with synthetic signed media: no object stands behind any URL here.
#![cfg(feature = "contract")]
use serde_json::{Value, json};
use veoveo_mcp_embedding::contract::*;

/// A synthetic signed URL, which no schema, result or error may echo.
const SIGNED_URL: &str = "https://storage.test/object?X-Amz-Signature=synthetic-secret";

fn request(options: Value) -> Result<EmbedRequest, serde_json::Error> {
    let mut value = json!({"items":[{"content":[{"type":"text","text":"hello again"}]}]});
    value
        .as_object_mut()
        .unwrap()
        .extend(options.as_object().unwrap().clone());
    serde_json::from_value(value)
}
fn media(kind: &str, media_type: &str, size: u64) -> Value {
    json!({"type":kind,"url":SIGNED_URL,"mediaType":media_type,"size":size})
}
#[test]
fn ordered_parts_dimensions_and_optional_native_prompt_are_closed() {
    let base = request(json!({})).unwrap();
    assert_eq!(base.effective_prompt(), "");
    assert_eq!(base.dimensions, Dimensions::D768);
    assert!(base.validate().is_ok());
    let item = json!({"content":[{"type":"text","text":"before"},media("image","image/png",3),{"type":"text","text":"after"}]});
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
        json!({"items":[{"content":[{"type":"image","artifactId":"019a0000-0000-7000-8000-000000000001"}]}]}),
        json!({"items":[{"content":[{"type":"image","url":SIGNED_URL}]}]}),
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
    // Refused on decoding: an item holds 1 to 16 parts.
    for value in [
        json!({"items":[{"content":[]}]}),
        json!({"items":[{"content":vec![json!({"type":"text","text":"a"});17]}]}),
    ] {
        assert!(request(value.clone()).is_err(), "{value}");
    }
    for value in [
        json!({"items":[]}),
        json!({"items":vec![json!({"content":[{"type":"text","text":"a"}]} );9]}),
        json!({"items":vec![json!({"content":vec![json!({"type":"text","text":"a"});11]});3]}),
        json!({"items":[{"content":[{"type":"text","text":"a".repeat(MAX_CONTENT_BYTES)}]}],"prompt":"é"}),
        json!({"items":[{"content":[{"type":"text","text":" "}]}]}),
        json!({"prompt":"<|video|>"}),
        json!({"preprocessing":{"image":{"maxSoftTokens":71}}}),
        json!({"preprocessing":{"audio":{"maxLength":1}}}),
        json!({"preprocessing":{"video":{"fps":0}}}),
        json!({"preprocessing":{"video":{"fps":121}}}),
        json!({"preprocessing":{"video":{"maxFrames":257}}}),
        json!({"items":[{"content":[media("audio","audio/wav",3)]}],"preprocessing":{"video":{"loadAudioFromVideo":true}}}),
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
fn media_bounds_hold_on_the_size_each_part_states() {
    let one = |size: u64| {
        request(json!({"items":[{"content":[media("image","image/png",size)]}]}))
            .unwrap()
            .validate()
    };
    assert!(one(MAX_SOURCE_BYTES).is_ok());
    assert_eq!(
        one(MAX_SOURCE_BYTES + 1),
        Err("a media part's stated size exceeds 64 MiB")
    );
    // Two whole sources fit the aggregate; a third byte beyond it does not.
    let parts = |sizes: &[u64]| json!({"items":[{"content":sizes.iter().map(|size| media("video","video/mp4",*size)).collect::<Vec<_>>()}]});
    assert!(
        request(parts(&[MAX_SOURCE_BYTES, MAX_SOURCE_BYTES]))
            .unwrap()
            .validate()
            .is_ok()
    );
    assert_eq!(
        request(parts(&[MAX_SOURCE_BYTES, MAX_SOURCE_BYTES, 1]))
            .unwrap()
            .validate(),
        Err("the media's stated sizes exceed 128 MiB in all")
    );
    for refused in [
        media("image", "image/png", 0),
        media(
            "image",
            "image/png",
            veoveo_hub_sdk::knowledge::MAX_MEDIA_BYTES + 1,
        ),
    ] {
        assert!(
            request(json!({"items":[{"content":[refused.clone()]}]})).is_err(),
            "{refused}"
        );
    }
}
#[test]
fn a_media_part_is_of_its_own_kind_and_audio_may_be_a_soundtrack() {
    for (kind, media_type, admitted) in [
        ("image", "image/png", true),
        ("image", "video/mp4", false),
        ("audio", "audio/wav", true),
        ("audio", "video/mp4", true),
        ("audio", "image/png", false),
        ("video", "video/mp4", true),
        ("video", "audio/wav", false),
        ("video", "application/octet-stream", false),
    ] {
        let decoded = request(json!({"items":[{"content":[media(kind,media_type,3)]}]}));
        assert_eq!(
            decoded.is_ok_and(|request| request.validate().is_ok()),
            admitted,
            "{kind} {media_type}"
        );
    }
    // The shape a receipt keeps is checked by the same rule.
    let mut shape = request(json!({"items":[{"content":[media("image","image/png",3)]}]}))
        .unwrap()
        .shape();
    shape.items[0].content[0] = PartShape::Image(MediaSource {
        media_type: MediaType::parse("audio/wav").unwrap(),
        size: MediaSize::new(3).unwrap(),
    });
    assert!(shape.validate().is_err());
}
#[test]
fn a_signed_url_is_input_only_and_never_reaches_output_debug_or_errors() {
    let input =
        json!({"items":[{"content":[{"type":"text","text":"a"},media("image","image/png",3)]}]});
    let decoded = request(input).unwrap();
    // The public input necessarily carries the signed URL; nothing else does.
    let schema = serde_json::to_value(schemars::schema_for!(EmbedRequest))
        .unwrap()
        .to_string();
    assert!(schema.contains("\"url\"") && schema.contains("mediaType"));
    for private in [
        "artifactId",
        "RuntimeSource",
        "RuntimeRequest",
        "expiresAt",
        "\"get\"",
    ] {
        assert!(!schema.contains(private), "{private}");
    }
    let output = serde_json::to_value(schemars::schema_for!(EmbeddingResult))
        .unwrap()
        .to_string();
    for private in ["\"url\"", "\"headers\"", "SignedUrl", "artifactId"] {
        assert!(!output.contains(private), "{private}");
    }
    assert!(!format!("{decoded:?}").contains("synthetic-secret"));
    let shape = serde_json::to_string(&decoded.shape()).unwrap();
    assert!(
        !shape.contains("synthetic-secret") && !shape.contains("\"url\""),
        "{shape}"
    );
    assert_eq!(
        decoded.shape().items[0].content[1].source(),
        Some(&MediaSource {
            media_type: MediaType::parse("image/png").unwrap(),
            size: MediaSize::new(3).unwrap(),
        })
    );
    // The contract's own refusals name their rule; over MCP the SDK never returns a
    // decoder's text at all.
    for refused in [
        json!({"items":[{"content":[{"type":"image","url":format!("ftp{}",&SIGNED_URL[5..]),"mediaType":"image/png","size":3}]}]}),
        json!({"items":[{"content":[media("image","image/png",3)]}],"extra":SIGNED_URL}),
    ] {
        let error = request(refused).unwrap_err().to_string();
        assert!(!error.contains("synthetic-secret"), "{error}");
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
