//! A request without its signed URLs and headers: what a result is validated against and
//! what a saved receipt keeps.
use serde::{Deserialize, Serialize};

use super::*;

/// A request as its result is validated against it: each item's ordered parts as text, or
/// a media part's modality, media type and size, and the request-wide options. It holds no
/// signed URL or header, so a receipt can keep it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestShape {
    pub items: Vec<ShapeItem>,
    #[serde(default)]
    pub dimensions: Dimensions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_name: Option<PromptName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default)]
    pub preprocessing: Preprocessing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShapeItem {
    pub content: Vec<PartShape>,
}

/// One part of a [`RequestShape`]: its text, or its media's type and size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum PartShape {
    Text { text: String },
    Image(MediaSource),
    Audio(MediaSource),
    Video(MediaSource),
}

impl PartShape {
    /// The shape of `part`: its signed URL and headers left out.
    pub fn of(part: &ContentPart) -> Self {
        let source = |media: &SignedMedia| MediaSource::of(media);
        match part {
            ContentPart::Text { text } => Self::Text { text: text.clone() },
            ContentPart::Image(media) => Self::Image(source(media)),
            ContentPart::Audio(media) => Self::Audio(source(media)),
            ContentPart::Video(media) => Self::Video(source(media)),
        }
    }

    pub fn modality(&self) -> Modality {
        match self {
            Self::Text { .. } => Modality::Text,
            Self::Image(_) => Modality::Image,
            Self::Audio(_) => Modality::Audio,
            Self::Video(_) => Modality::Video,
        }
    }

    /// The media a media part states.
    pub fn source(&self) -> Option<&MediaSource> {
        match self {
            Self::Text { .. } => None,
            Self::Image(source) | Self::Audio(source) | Self::Video(source) => Some(source),
        }
    }
}

impl RequestShape {
    pub fn effective_prompt(&self) -> &str {
        self.prompt
            .as_deref()
            .unwrap_or_else(|| self.prompt_name.map_or("", PromptName::prefix))
    }

    /// Checks the service bounds: 1 to 8 items of 1 to 16 parts, at most 32 parts and
    /// 64 KiB of text and prompt, nonblank text without media placeholders, each media part
    /// of its own kind and at most 64 MiB, at most 128 MiB of media in all on the sizes the
    /// parts state, and no soundtrack beside separate audio. Returns the text bytes.
    pub fn validate(&self) -> Result<usize, &'static str> {
        self.preprocessing.validate()?;
        if self.prompt.is_some() && self.prompt_name.is_some() {
            return Err("prompt and promptName are mutually exclusive");
        }
        if self.items.is_empty() || self.items.len() > MAX_ITEMS {
            return Err("batch must contain 1 to 8 items");
        }
        let mut bytes = self.effective_prompt().len();
        let mut parts = 0;
        let mut media = 0_u64;
        let mut has_audio = false;
        if reserved(self.effective_prompt()) {
            return Err("media placeholders are reserved for ordered content parts");
        }
        for item in &self.items {
            if item.content.is_empty() || item.content.len() > MAX_PARTS_PER_ITEM {
                return Err("item must contain 1 to 16 ordered parts");
            }
            parts += item.content.len();
            for part in &item.content {
                match part {
                    PartShape::Text { text } => {
                        if text.trim().is_empty() || reserved(text) {
                            return Err(
                                "text must be nonblank and contain no reserved media placeholders",
                            );
                        }
                        bytes = bytes
                            .checked_add(text.len())
                            .ok_or("content size overflow")?;
                    }
                    PartShape::Image(source)
                    | PartShape::Audio(source)
                    | PartShape::Video(source) => {
                        if !part.modality().admits(&source.media_type) {
                            return Err("a media part's media type is of its type");
                        }
                        if source.size.bytes() > MAX_SOURCE_BYTES {
                            return Err("a media part's stated size exceeds 64 MiB");
                        }
                        media = media
                            .checked_add(source.size.bytes())
                            .ok_or("media size overflow")?;
                    }
                }
                has_audio |= matches!(part, PartShape::Audio(_));
            }
        }
        if parts > MAX_PARTS || bytes > MAX_CONTENT_BYTES {
            return Err("batch content exceeds service bounds");
        }
        if media > MAX_TRANSFER_BYTES {
            return Err("the media's stated sizes exceed 128 MiB in all");
        }
        if has_audio && self.preprocessing.video.load_audio_from_video {
            return Err("video soundtrack inclusion cannot be combined with separate audio parts");
        }
        Ok(bytes)
    }
}
