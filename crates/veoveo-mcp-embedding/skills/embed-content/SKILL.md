---
name: embed-content
description: Turn text, images, audio and video into normalized vectors with one fixed multimodal model, one joint vector per item, for search, clustering, classification or similarity. Use when a task compares, ranks or groups content by meaning.
---

# Embed content

`embedding__embed` runs one fixed model, EmbeddingGemma 2, so vectors from different calls
share one space. Read `embedding://capabilities` for its native controls, defaults and the
service bounds before a large request.

## A request

- `items`: up to 8 ordered items. Each item's `content` is an ordered list of parts, up to
  16, and all of one item's parts make one joint vector. A part is
  `{"type": "text", "text": ...}`, or
  `{"type": "image" | "audio" | "video", "url": ..., "headers": {...}, "mediaType": ..., "size": ...}`
  for media signed for the person. To embed an artifact, first call
  `artifact__create_download` for it and pass the `url` and `headers` of its `get`, and the
  artifact's `mediaType` and `size`. A signed URL lives 5 minutes, so sign right before the
  call. An image's media type is `image/*`, a video's `video/*`, and audio's `audio/*` or a
  video's soundtrack, `video/*`. Code is ordinary text.
- `dimensions`: 768 by default, or 128, 256 or 512 for a shorter vector of the same space.
  Use one dimension for everything you compare.
- `promptName` or `prompt`, never both, once for the whole request: a named purpose
  (`SearchQuery`, `CodeRetrieval`, `Document`, `QuestionAnswering`, `FactChecking`,
  `Classification`, `Clustering`, `SentenceSimilarity`) or your own prefix. Embed queries
  with `SearchQuery` and the documents they search with `Document`.
- `preprocessing`: image soft tokens (280 by default), audio truncation (off by default:
  long audio is refused rather than silently cut), and video sampling (1 frame per second,
  at most 32 frames). The answer reports what sampling or truncation applied.

## The answer

One row per item, in order: its `index`, the normalized `vector`, its `tokenCount`, and for
each media part, by its `partIndex`, its `mediaType` and `size` and what was read. Nothing
is stored: content and vectors exist only in this answer, so
keep the vectors you need. A retry computes again and is billed again.

## Limits

A request over the service bounds, or whose tokens exceed 8192 per item or 16384 per
request, is refused before the model runs: a media part's stated `size` is at most 64 MiB,
and 128 MiB in all. Media the runtime cannot fetch, such as an expired signed URL, is
refused with nothing computed; sign it again. Split large inputs across requests and items.

A signed URL and its headers are the person's credential while they live: never paste
one into chat, a report or a note.
