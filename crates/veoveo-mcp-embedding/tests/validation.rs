//! Synthetic output/coverage fixtures and frozen Python-to-Rust JCS vectors.
#![cfg(feature = "server")]
mod support;
use serde_json::{Value, json};
use veoveo_mcp_artifact::contract::{
    MediaType, ObjectSize, SignedTransfer, SignedUrl, TransferMethod,
};
use veoveo_mcp_embedding::{
    contract::*,
    protocol::*,
    provider::{validate_capabilities, validate_result},
};
use veoveo_types::Sha256Digest;
fn source(kind: &str) -> RuntimeSource {
    RuntimeSource {
        artifact_id: ArtifactId::parse(uuid::Uuid::now_v7().to_string()).unwrap(),
        media_type: MediaType::parse(format!("{kind}/synthetic")).unwrap(),
        size: ObjectSize::new(3).unwrap(),
        get: SignedTransfer {
            method: TransferMethod::Get,
            url: SignedUrl::parse("http://private.test:9000/object?synthetic-secret=do-not-log")
                .unwrap(),
            headers: Default::default(),
            expires_at: chrono::Utc::now() + chrono::TimeDelta::minutes(5),
        },
    }
}
fn joint() -> (RuntimeRequest, EmbeddingResult) {
    let mut input = support::input();
    input.preprocessing.video.fps = 2.0;
    let mut parts = vec![RuntimePart::Text {
        text: "before".into(),
    }];
    for _ in 0..2 {
        parts.extend([
            RuntimePart::Image {
                source: source("image"),
            },
            RuntimePart::Audio {
                source: source("audio"),
            },
            RuntimePart::Video {
                source: source("video"),
            },
        ]);
    }
    parts.push(RuntimePart::Text {
        text: "after".into(),
    });
    let request = RuntimeRequest::new(
        uuid::Uuid::now_v7(),
        &input,
        vec![RuntimeItem { content: parts }],
    );
    let mut output = support::output(&request);
    output.items[0].media = request.items[0]
        .content
        .iter()
        .enumerate()
        .filter_map(|(part_index, part)| {
            part.source().map(|source| match part {
                RuntimePart::Image { .. } => MediaCoverage::Image {
                    part_index,
                    source: source.identity(),
                    coverage: ImageCoverage {
                        width: 10,
                        height: 20,
                        frame_index: 0,
                    },
                },
                RuntimePart::Audio { .. } => MediaCoverage::Audio {
                    part_index,
                    source: source.identity(),
                    coverage: AudioCoverage {
                        source_duration_seconds: 2.0,
                        source_sampling_rate: 48000,
                        source_channels: 2,
                        sampling_rate: 16000,
                        decoded_samples: 32000,
                        retained_samples: 32000,
                        truncated: false,
                    },
                },
                RuntimePart::Video { .. } => MediaCoverage::Video {
                    part_index,
                    source: source.identity(),
                    coverage: VideoCoverage {
                        source_duration_seconds: 2.0,
                        source_frame_count: 2,
                        source_fps: 1.0,
                        width: 10,
                        height: 20,
                        frame_indices: vec![0, 0, 1, 1],
                        frame_timestamps_seconds: vec![10.0, 10.0, 11.0, 11.0],
                        model_frame_timestamps_seconds: vec![0.0, 0.0, 1.0, 1.0],
                        audio_included: false,
                        audio: None,
                    },
                },
                RuntimePart::Text { .. } => unreachable!(),
            })
        })
        .collect();
    (request, output)
}
#[test]
fn frozen_python_jcs_bytes_match_rust_for_unicode_and_numeric_equivalence() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/processing-jcs.json")).unwrap();
    let mut hashes = vec![];
    for case in fixture["cases"].as_array().unwrap() {
        let canonical = serde_json_canonicalizer::to_vec(&case["descriptor"]).unwrap();
        assert_eq!(
            canonical,
            case["canonicalUtf8"].as_str().unwrap().as_bytes()
        );
        assert_eq!(
            digest(&canonical).as_str(),
            case["sha256"].as_str().unwrap()
        );
        let descriptor = &case["descriptor"];
        let mut input = support::input();
        input.dimensions = serde_json::from_value(descriptor["dimensions"].clone()).unwrap();
        input.prompt = Some(descriptor["prompt"].as_str().unwrap().into());
        input.preprocessing = serde_json::from_value(descriptor["preprocessing"].clone()).unwrap();
        let request = support::text_request(&input);
        assert_eq!(
            serde_json_canonicalizer::to_vec(&request.processing_descriptor()).unwrap(),
            canonical
        );
        assert_eq!(
            request.processing_id().unwrap().as_str(),
            case["sha256"].as_str().unwrap()
        );
        hashes.push(case["sha256"].clone());
    }
    assert_eq!(hashes[0], hashes[1]);
    assert_ne!(hashes[0], hashes[2]);
}
#[test]
fn media_source_contract_has_no_inherited_content_checksum() {
    let source = source("image");
    let mut private = serde_json::to_value(&source).unwrap();
    let public = serde_json::to_value(source.identity()).unwrap();
    assert!(private.get("sha256").is_none());
    assert!(public.get("sha256").is_none());
    private["sha256"] = json!(Sha256Digest::from_bytes([0; 32]));
    assert!(serde_json::from_value::<RuntimeSource>(private).is_err());
    let mut old_public = public;
    old_public["sha256"] = json!(Sha256Digest::from_bytes([0; 32]));
    assert!(serde_json::from_value::<MediaSource>(old_public).is_err());
    let schema = serde_json::to_value(schemars::schema_for!(MediaSource)).unwrap();
    assert!(schema["properties"].get("sha256").is_none());
}

#[test]
fn repeated_modalities_and_repeated_native_indices_keep_joint_order_and_private_boundary() {
    let (request, result) = joint();
    request.validate().unwrap();
    validate_result(&result, &request, &Device::Mps).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].media.len(), 6);
    assert_eq!(
        result.items[0]
            .media
            .iter()
            .map(MediaCoverage::part_index)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6]
    );
    let public = serde_json::to_string(&result).unwrap();
    for forbidden in [
        "synthetic-secret",
        "SignedTransfer",
        "expiresAt",
        "\"get\"",
        "\"url\"",
    ] {
        assert!(!public.contains(forbidden));
    }
    assert!(!format!("{request:?}").contains("before"));
    assert!(!format!("{:?}", request.items[0].content[1].source()).contains("synthetic-secret"));
}
#[test]
fn tampered_media_processing_identity_coverage_cardinality_and_order_are_refused() {
    let (request, baseline) = joint();
    for change in 0..15 {
        let mut result = baseline.clone();
        match change {
            0 => result.processing_id = Sha256Digest::from_bytes([9; 32]),
            1 => result.processing.prompt_sha256 = Sha256Digest::from_bytes([9; 32]),
            2 => result.processing.preprocessing.video.fps = 3.0,
            3 => {
                result.items[0].media.pop();
            }
            4 => result.items[0].media.swap(0, 3),
            5 => {
                if let MediaCoverage::Image { coverage, .. } = &mut result.items[0].media[0] {
                    coverage.width = MAX_PIXELS + 1;
                }
            }
            6 => {
                if let MediaCoverage::Audio { coverage, .. } = &mut result.items[0].media[1] {
                    coverage.retained_samples = 1;
                }
            }
            7 => {
                if let MediaCoverage::Video { coverage, .. } = &mut result.items[0].media[2] {
                    coverage.frame_indices = vec![0, 1, 0, 1];
                }
            }
            8 => {
                if let MediaCoverage::Video { coverage, .. } = &mut result.items[0].media[2] {
                    coverage.model_frame_timestamps_seconds[0] = 1.0;
                }
            }
            9 => {
                if let MediaCoverage::Video { coverage, .. } = &mut result.items[0].media[2] {
                    coverage.audio_included = true;
                }
            }
            10 => {
                if let MediaCoverage::Audio { source, .. } = &mut result.items[0].media[1] {
                    source.size = ObjectSize::new(4).unwrap();
                }
            }
            11 => {
                if let MediaCoverage::Video { coverage, .. } = &mut result.items[0].media[2] {
                    coverage.frame_timestamps_seconds[0] = f64::NAN;
                }
            }
            12 => result.runtime.torch = "2.14.1+cu130".into(),
            13 => result.runtime.torchcodec = "0.17.0+cpu".into(),
            _ => result.runtime.ffmpeg.libavcodec[0] = 0,
        }
        assert!(
            validate_result(&result, &request, &Device::Mps).is_err(),
            "change{change}"
        );
    }
}
#[test]
fn exact_capabilities_defaults_packages_and_service_limits_are_required() {
    let base = support::capabilities();
    validate_capabilities(&base, &Device::Mps).unwrap();
    for change in 0..7 {
        let mut caps = base.clone();
        match change {
            0 => caps.defaults.audio.truncation = true,
            1 => caps.modalities.reverse(),
            2 => caps.prompt_names.pop().map(|_| ()).unwrap(),
            3 => caps.execution.batch_size = 2,
            4 => caps.limits.max_parts += 1,
            5 => caps.supported_dimensions.reverse(),
            _ => caps.runtime.torch = "2.14.1+other".into(),
        }
        assert!(validate_capabilities(&caps, &Device::Mps).is_err());
    }
    let mut cuda = base;
    cuda.execution.device = Device::Cuda;
    cuda.runtime.torch = "2.14.1+cu130".into();
    cuda.runtime.torchvision = "0.29.1+cu130".into();
    cuda.runtime.torchcodec = "0.17.0+cpu".into();
    validate_capabilities(&cuda, &Device::Cuda).unwrap();
    let mut bad = json!(cuda);
    bad["runtime"]["ffmpeg"]["libavcodec"] = json!([62, 1]);
    assert!(serde_json::from_value::<Capabilities>(bad).is_err());
}

/// Producer/consumer smoke over externally retained real Python output. This test
/// only reads a closed local fixture; it never starts a model or reaches a service.
#[test]
#[ignore = "requires coordinator-supplied safe real Python capability/response fixture"]
fn actual_python_wire_is_accepted_by_rust_without_another_model_call() {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct ExpectedSource {
        part_index: usize,
        source: MediaSource,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        capabilities: Capabilities,
        input: EmbedRequest,
        sources: Vec<Vec<ExpectedSource>>,
        output: EmbeddingResult,
    }
    let path =
        std::env::var_os("VEOVEO_EMBEDDING_WIRE_FIXTURE").expect("external wire fixture required");
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() <= MAX_RESPONSE_BYTES * 2);
    let fixture: Wire = serde_json::from_slice(&bytes).unwrap();
    let structured: Wire =
        serde_json::from_value(serde_json::from_slice::<Value>(&bytes).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&fixture.output).unwrap(),
        serde_json::to_value(&structured.output).unwrap()
    );
    let sources: Vec<Vec<_>> = fixture
        .sources
        .into_iter()
        .map(|rows| {
            rows.into_iter()
                .map(|row| (row.part_index, row.source))
                .collect()
        })
        .collect();
    validate_capabilities(&fixture.capabilities, &fixture.output.execution.device).unwrap();
    veoveo_mcp_embedding::provider::validate_public_result(
        &fixture.output,
        fixture.output.request_id,
        &fixture.input,
        &sources,
        &fixture.output.execution.device,
    )
    .unwrap();
}

#[test]
fn actual_decoded_audio_aggregate_is_bounded_despite_underreported_source_metadata() {
    for soundtrack in [false, true] {
        let mut input = support::input();
        input.preprocessing.video.load_audio_from_video = soundtrack;
        let parts = (0..3)
            .map(|_| {
                if soundtrack {
                    RuntimePart::Video {
                        source: source("video"),
                    }
                } else {
                    RuntimePart::Audio {
                        source: source("audio"),
                    }
                }
            })
            .collect();
        let request = RuntimeRequest::new(
            uuid::Uuid::now_v7(),
            &input,
            vec![RuntimeItem { content: parts }],
        );
        let mut result = support::output(&request);
        result.items[0].media = request.items[0]
            .content
            .iter()
            .enumerate()
            .map(|(part_index, part)| {
                let audio = AudioCoverage {
                    source_duration_seconds: 1.0,
                    source_sampling_rate: 16000,
                    source_channels: 1,
                    sampling_rate: 16000,
                    decoded_samples: 200 * 16000,
                    retained_samples: 200 * 16000,
                    truncated: false,
                };
                let source = part.source().unwrap().identity();
                if soundtrack {
                    MediaCoverage::Video {
                        part_index,
                        source,
                        coverage: VideoCoverage {
                            source_duration_seconds: 1.0,
                            source_frame_count: 1,
                            source_fps: 1.0,
                            width: 1,
                            height: 1,
                            frame_indices: vec![0],
                            frame_timestamps_seconds: vec![0.0],
                            model_frame_timestamps_seconds: vec![0.0],
                            audio_included: true,
                            audio: Some(audio),
                        },
                    }
                } else {
                    MediaCoverage::Audio {
                        part_index,
                        source,
                        coverage: audio,
                    }
                }
            })
            .collect();
        validate_result(&result, &request, &Device::Mps).unwrap();
        let audio = match &mut result.items[0].media[2] {
            MediaCoverage::Audio { coverage, .. } => coverage,
            MediaCoverage::Video { coverage, .. } => coverage.audio.as_mut().unwrap(),
            _ => unreachable!(),
        };
        audio.decoded_samples += 1;
        audio.retained_samples += 1;
        assert!(
            validate_result(&result, &request, &Device::Mps).is_err(),
            "soundtrack={soundtrack}"
        );
    }
}

#[test]
fn fractional_media_coverage_decodes_from_wire_and_sdk_structured_content() {
    let (_, mut result) = joint();
    if let MediaCoverage::Audio { coverage, .. } = &mut result.items[0].media[1] {
        coverage.source_duration_seconds = 1.23456789;
    }
    if let MediaCoverage::Video { coverage, .. } = &mut result.items[0].media[2] {
        coverage.source_duration_seconds = 1.23456789;
        coverage.source_fps = 29.97002997;
        coverage.frame_timestamps_seconds =
            vec![0.123456789, 0.123456789, 1.123456789, 1.123456789];
    }
    let bytes = serde_json::to_vec(&result).unwrap();
    let wire: EmbeddingResult = serde_json::from_slice(&bytes).unwrap();
    let structured: EmbeddingResult =
        serde_json::from_value(serde_json::from_slice::<Value>(&bytes).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&wire).unwrap(),
        serde_json::to_value(&structured).unwrap()
    );
    assert_eq!(wire.items[0].media, result.items[0].media);
    let mut bad = serde_json::to_value(&result).unwrap();
    bad["items"][0]["media"][1]["coverage"]["unexpected"] = json!(true);
    assert!(serde_json::from_value::<EmbeddingResult>(bad).is_err());
}
