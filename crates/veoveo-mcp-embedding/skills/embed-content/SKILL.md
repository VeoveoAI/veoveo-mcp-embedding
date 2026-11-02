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
  `{"type": "text", "text": ...}`, or `{"type": "image" | "audio" | "video", "artifactId": ...}`
  for media stored in Artifact, which the person must be able to read. Code is ordinary
  text.
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
media what was read. Nothing is stored: content and vectors exist only in this answer, so
keep the vectors you need. A retry computes again and is billed again.

## Limits

A request over the service bounds, or whose tokens exceed 8192 per item or 16384 per
request, is refused before the model runs, and so is a media reference the person cannot
read. Split large inputs across requests and items.
