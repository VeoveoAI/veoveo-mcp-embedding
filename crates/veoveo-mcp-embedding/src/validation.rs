//! Independent closed validation of runtime identity, processing and ordered output.
use crate::{contract::*, protocol::RuntimeRequest, provider::RuntimeError};

fn valid() -> RuntimeError {
    RuntimeError::InvalidResponse
}
fn identity(
    model: &ModelIdentity,
    runtime: &RuntimeIdentity,
    device: &Device,
) -> Result<(), RuntimeError> {
    let (torch, vision, codec) = match device {
        Device::Mps => ("2.14.1", "0.29.1", "0.17.0"),
        Device::Cuda => ("2.14.1+cu130", "0.29.1+cu130", "0.17.0+cpu"),
    };
    let ffmpeg = &runtime.ffmpeg;
    if model.id != MODEL_ID
        || model.revision != MODEL_REVISION
        || runtime.package != "veoveo-runtime-embedding"
        || runtime.version != "0.1.0"
        || runtime.python != "3.14.6"
        || runtime.sentence_transformers != "6.1.0"
        || runtime.transformers != "5.20.2"
        || runtime.torch != torch
        || runtime.torchvision != vision
        || runtime.torchcodec != codec
        || runtime.pillow != "13.0.0"
        || runtime.source_revision.len() != 40
        || !runtime
            .source_revision
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || ffmpeg.version.is_empty()
        || ffmpeg.version.len() > 128
        || !ffmpeg.version.starts_with(|c: char| c.is_ascii_digit())
        || ffmpeg.version.chars().any(char::is_control)
        || [
            ffmpeg.libavfilter,
            ffmpeg.libavutil,
            ffmpeg.libavcodec,
            ffmpeg.libavformat,
        ]
        .iter()
        .any(|version| version[0] == 0)
    {
        return Err(valid());
    }
    Ok(())
}
pub fn validate_capabilities(value: &Capabilities, device: &Device) -> Result<(), RuntimeError> {
    identity(&value.model, &value.runtime, device)?;
    if value.dimensions != Dimensions::D768
        || value.supported_dimensions
            != [
                Dimensions::D128,
                Dimensions::D256,
                Dimensions::D512,
                Dimensions::D768,
            ]
        || !value.normalized
        || value.modalities
            != [
                Modality::Text,
                Modality::Image,
                Modality::Audio,
                Modality::Video,
            ]
        || value.prompt_names != PromptName::ALL
        || !value.default_prompt.is_empty()
        || value.defaults != Preprocessing::default()
        || &value.execution.device != device
        || value.execution.dtype != "float32"
        || value.execution.batch_size != 1
        || value.limits != Limits::default()
        || value.processing_semantics != ProcessingSemantics::default()
    {
        return Err(valid());
    }
    Ok(())
}
fn pixels(width: u64, height: u64) -> Result<u64, RuntimeError> {
    let value = width.checked_mul(height).ok_or_else(valid)?;
    if width == 0 || height == 0 || value > MAX_PIXELS {
        return Err(valid());
    }
    Ok(value)
}
fn duration(seconds: f64) -> Result<f64, RuntimeError> {
    if !seconds.is_finite() || seconds <= 0.0 || seconds > MAX_SOURCE_SECONDS as f64 {
        return Err(valid());
    }
    Ok(seconds)
}
fn audio(value: &AudioCoverage, options: &AudioOptions) -> Result<f64, RuntimeError> {
    let seconds = duration(value.source_duration_seconds)?;
    let retained = if options.truncation {
        value
            .decoded_samples
            .min(options.max_length.unwrap_or(480000))
    } else {
        value.decoded_samples
    };
    if value.source_sampling_rate == 0
        || value.source_channels == 0
        || value.sampling_rate != 16000
        || value.decoded_samples == 0
        || value.decoded_samples > MAX_SOURCE_SECONDS * 16000
        || value.retained_samples != retained
        || value.truncated != (retained < value.decoded_samples)
    {
        return Err(valid());
    }
    Ok(seconds)
}
fn media(value: &MediaCoverage, options: &Preprocessing) -> Result<(u64, f64, u64), RuntimeError> {
    match value {
        MediaCoverage::Image { coverage, .. } => {
            if coverage.frame_index != 0 {
                return Err(valid());
            }
            Ok((pixels(coverage.width, coverage.height)?, 0.0, 0))
        }
        MediaCoverage::Audio { coverage, .. } => Ok((
            0,
            audio(coverage, &options.audio)?,
            coverage.decoded_samples,
        )),
        MediaCoverage::Video { coverage, .. } => {
            duration(coverage.source_duration_seconds)?;
            let frames = coverage.frame_indices.len();
            if coverage.source_frame_count == 0
                || !coverage.source_fps.is_finite()
                || coverage.source_fps <= 0.0
                || frames == 0
                || frames > options.video.max_frames
                || frames > MAX_FRAMES
                || coverage.frame_timestamps_seconds.len() != frames
                || coverage.model_frame_timestamps_seconds.len() != frames
                || coverage
                    .frame_indices
                    .iter()
                    .any(|index| *index >= coverage.source_frame_count)
                || coverage
                    .frame_indices
                    .windows(2)
                    .any(|pair| pair[0] > pair[1])
                || coverage
                    .frame_timestamps_seconds
                    .iter()
                    .any(|t| !t.is_finite())
                || coverage
                    .frame_timestamps_seconds
                    .windows(2)
                    .any(|pair| pair[0] > pair[1])
                || coverage.audio_included != options.video.load_audio_from_video
                || coverage.audio.is_some() != coverage.audio_included
            {
                return Err(valid());
            }
            for (index, timestamp) in coverage
                .frame_indices
                .iter()
                .zip(&coverage.model_frame_timestamps_seconds)
            {
                let expected = *index as f64 / coverage.source_fps;
                if !timestamp.is_finite()
                    || !expected.is_finite()
                    || (*timestamp - expected).abs() > 1e-9 * expected.abs().max(1.0)
                {
                    return Err(valid());
                }
            }
            // Actual codec PTS can have a source timeline offset. Do not confuse
            // them with the processor's separate index/FPS prompt timestamps.
            let samples = coverage
                .audio
                .as_ref()
                .map(|value| audio(value, &options.audio))
                .transpose()?
                .unwrap_or(0.0);
            Ok((
                pixels(coverage.width, coverage.height)?
                    .checked_mul(frames as u64)
                    .ok_or_else(valid)?,
                samples,
                coverage
                    .audio
                    .as_ref()
                    .map_or(0, |audio| audio.decoded_samples),
            ))
        }
    }
}
pub fn validate_result(
    value: &EmbeddingResult,
    request: &RuntimeRequest,
    device: &Device,
) -> Result<(), RuntimeError> {
    let sources: Vec<Vec<_>> = request
        .items
        .iter()
        .map(|item| {
            item.content
                .iter()
                .enumerate()
                .filter_map(|(index, part)| part.source().map(|source| (index, source.identity())))
                .collect()
        })
        .collect();
    validate_public_result(
        value,
        request.request_id,
        &request.public(),
        &sources,
        device,
    )
}

/// Validate public output against exact public input and caller-authorized safe
/// Artifact identities. Useful for replay-free receipt validation, without grants.
pub fn validate_public_result(
    value: &EmbeddingResult,
    request_id: uuid::Uuid,
    request: &EmbedRequest,
    sources: &[Vec<(usize, MediaSource)>],
    device: &Device,
) -> Result<(), RuntimeError> {
    request.validate().map_err(|_| valid())?;
    identity(&value.model, &value.runtime, device)?;
    let dimensions: usize = request.dimensions.into();
    if value.request_id != request_id
        || value.dimensions != request.dimensions
        || !value.normalized
        || &value.execution.device != device
        || value.execution.dtype != "float32"
        || value.items.len() != request.items.len()
        || sources.len() != request.items.len()
        || value.processing_id != request.processing_id().map_err(|_| valid())?
        || value.processing != request.effective_processing()
    {
        return Err(valid());
    }
    let (mut tokens, mut total_pixels, mut audio_seconds, mut source_bytes, mut decoded_samples) =
        (0_u64, 0_u64, 0.0_f64, 0_u64, 0_u64);
    for (index, (item, input)) in value.items.iter().zip(&request.items).enumerate() {
        if item.index != index
            || item.vector.len() != dimensions
            || item.token_count == 0
            || item.token_count > MAX_ITEM_TOKENS
            || item.vector.iter().any(|n| !n.is_finite())
        {
            return Err(valid());
        }
        let norm = item
            .vector
            .iter()
            .map(|n| f64::from(*n).powi(2))
            .sum::<f64>()
            .sqrt();
        if (norm - 1.0).abs() > 0.001 {
            return Err(valid());
        }
        tokens = tokens.checked_add(item.token_count).ok_or_else(valid)?;
        let expected: Vec<_> = input
            .content
            .iter()
            .enumerate()
            .filter_map(|(i, part)| part.artifact_id().map(|id| (i, part.modality(), id)))
            .collect();
        if expected.len() != item.media.len() || sources[index].len() != expected.len() {
            return Err(valid());
        }
        for (((part_index, kind, id), (source_index, source)), row) in
            expected.into_iter().zip(&sources[index]).zip(&item.media)
        {
            let observed = match row {
                MediaCoverage::Image { .. } => Modality::Image,
                MediaCoverage::Audio { .. } => Modality::Audio,
                MediaCoverage::Video { .. } => Modality::Video,
            };
            if row.part_index() != part_index
                || kind != observed
                || *source_index != part_index
                || source.artifact_id != id
                || row.source() != source
            {
                return Err(valid());
            }
            let mime = source.media_type.as_str();
            let family = match kind {
                Modality::Image => mime.starts_with("image/"),
                Modality::Audio => mime.starts_with("audio/") || mime.starts_with("video/"),
                Modality::Video => mime.starts_with("video/"),
                Modality::Text => false,
            };
            if !family || source.size.bytes() == 0 || source.size.bytes() > MAX_ARTIFACT_BYTES {
                return Err(valid());
            }
            source_bytes = source_bytes
                .checked_add(source.size.bytes())
                .ok_or_else(valid)?;
            let (count, seconds, samples) = media(row, &request.preprocessing.effective())?;
            total_pixels = total_pixels.checked_add(count).ok_or_else(valid)?;
            audio_seconds += seconds;
            decoded_samples = decoded_samples.checked_add(samples).ok_or_else(valid)?;
        }
    }
    if tokens > MAX_BATCH_TOKENS
        || tokens != value.usage.input_tokens
        || total_pixels > MAX_TOTAL_PIXELS
        || source_bytes > MAX_TRANSFER_BYTES
        || !audio_seconds.is_finite()
        || audio_seconds > MAX_AUDIO_SECONDS as f64
        || decoded_samples > MAX_AUDIO_SECONDS * 16000
    {
        return Err(valid());
    }
    Ok(())
}
