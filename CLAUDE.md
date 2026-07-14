# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Purpose

`y7dl` is a Rust port of the Go library [kkdai/youtube](https://github.com/kkdai/youtube). It is a **library-first** crate (Rust edition 2024) whose scope is strictly:

1. **Extract video info** from a YouTube link: available formats with their `itag`, `quality`, and `mimeType`.
2. **Download a video stream** from a link at a caller-specified quality.

**Out of scope:** transcoding/muxing, playlists, captions/transcripts, and any CLI beyond a thin demo binary. Do not add these unless explicitly asked.

## Dependency Policy

Do not add third-party crates unless genuinely needed. The pre-approved set is: `tokio`, `serde`, `serde_json`, `reqwest`, `hyper`. Prefer `reqwest` over raw `hyper` for HTTP. Anything outside this list requires asking the user first. Prefer std and hand-rolled code over pulling in a crate (e.g. parse URLs/query strings manually rather than adding the `url` crate).

## Error Handling

Follow the agreed convention: a single crate-wide error enum (e.g. `Error` in `src/error.rs`) with variants for each failure class (video ID parse failure, HTTP/network errors, API response shape errors, cipher/decipher failures, unavailable/age-restricted videos, format-not-found, etc.). Expose `pub type Result<T> = std::result::Result<T, Error>;` and return it from all fallible public APIs. Implement `std::fmt::Display` and `std::error::Error` by hand, plus `From` impls for wrapped errors (`reqwest::Error`, `serde_json::Error`, `std::io::Error`). Do **not** add `thiserror`/`anyhow` — hand-write the impls.

## Commands

- `cargo build` — build
- `cargo test` — run all tests
- `cargo test <name>` — run a single test (substring match on test fn name)
- `cargo test -- --nocapture` — show `println!` output from tests
- `cargo clippy` — lint
- `cargo fmt` — format
- `cargo run` — run the demo binary

Note: tests that hit the live YouTube API are network-dependent and flaky by nature; keep parsing logic testable offline with fixture JSON where possible.

## Architecture (mirrors the Go original)

The crate is currently a fresh `cargo new` skeleton (`src/main.rs` only). Structure it as a library (`src/lib.rs`) with a thin `main.rs` demo. The Go original's components map to these intended modules:

- **`client`** (Go: `client.go`) — the public entry point. `Client` owns a `reqwest::Client` and exposes `get_video(url_or_id) -> Result<Video>` and stream-download methods. Fetches video metadata by POSTing to YouTube's InnerTube API (`https://www.youtube.com/youtubei/v1/player`) with a client context (the Go lib impersonates the Android/Web clients — client name, version, and API key in the request body).
- **`video` / `format`** (Go: `video.go`, `format_list.go`, `response_data.go`) — `serde` structs mirroring the InnerTube player response JSON (`streamingData.formats` and `streamingData.adaptiveFormats`). `Format` carries `itag`, `quality`/`qualityLabel`, `mimeType`, `bitrate`, `contentLength`, `url`, and optional `signatureCipher`. `Video` holds metadata plus a format list with helper filters (by itag, by quality, audio/video only, sorted best-first).
- **`decipher`** (Go: `decipher.go`, `decipher_operations.go`, `player_cache.go`) — formats without a plain `url` carry a `signatureCipher` that must be deciphered: download the player JS (`base.js`), extract the signature-transform operations (reverse/splice/swap) and the throttling `n`-parameter transform by regex, apply them, and rebuild the stream URL. Cache the parsed player per player-version since fetching/parsing `base.js` is expensive.
- **Download path** — streams are downloaded with HTTP `Range` requests in chunks (large files fail without ranged requests); write to an `impl std::io::Write`/`tokio` writer supplied by the caller.

Flow: URL → extract video ID (regex/string parsing) → InnerTube `player` request → deserialize player response → build `Video` with `Format` list → caller picks a format by quality/itag → decipher stream URL if needed → chunked ranged download.
