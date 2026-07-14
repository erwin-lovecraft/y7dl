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

- **`client`** (Go: `client.go`) — the public entry point. `Client` owns a `reqwest::Client` and exposes `get_video(url_or_id) -> Result<Video>`, `stream_url`, and `download`/`download_to_file`. Fetches video metadata by POSTing to YouTube's InnerTube API (`https://www.youtube.com/youtubei/v1/player`). **Important:** it impersonates the `ANDROID_VR` client, not `ANDROID`/`IOS` like the Go original — since late 2024 InnerTube rejects ANDROID/IOS requests without attestation tokens (HTTP 400 `FAILED_PRECONDITION`), and the deprecated `?key=` API-key param also triggers that error. ANDROID_VR responses carry direct (un-ciphered) stream URLs. Verified working 2026-07; if it breaks, probe other InnerTube clients the way yt-dlp does. Some videos answer `LOGIN_REQUIRED: Sign in to confirm you're not a bot` on the first anonymous request (per-video enforcement — all clients affected); the fix is to echo back the `responseContext.visitorData` token from that refused response (in `context.client.visitorData` + `X-Goog-Visitor-Id` header) and retry once — `Client` caches the token and does this automatically in `get_video`.
- **`video` / `format`** (Go: `video.go`, `format_list.go`, `response_data.go`) — `serde` structs mirroring the InnerTube player response JSON (`streamingData.formats` and `streamingData.adaptiveFormats`). `Format` carries `itag`, `quality`/`qualityLabel`, `mimeType`, `bitrate`, `contentLength`, `url`, and optional `signatureCipher`. `Video` holds metadata plus a format list with helper filters (by itag, by quality, audio/video only, sorted best-first).
- **`decipher`** (Go: `decipher.go`, `decipher_operations.go`, `player_cache.go`) — formats without a plain `url` carry a `signatureCipher`: download the player JS (embed page → `/s/player/<ver>/.../base.js`), extract the signature-transform ops (reverse/splice/swap) with hand-rolled string parsing, apply them, rebuild the stream URL; parsed ops are cached per player version. **Known limitation:** this only works for classic-format players. 2025+ players pack the cipher code in a runtime-decoded string table, and the `n`-parameter throttle transform likewise requires executing JS (the Go original embeds the goja JS interpreter for this) — both are out of scope under the dependency policy, and such formats yield `Error::Cipher`. The default ANDROID_VR path never needs deciphering, so this is a fallback only.
- **Download path** — streams are downloaded with HTTP `Range` requests in chunks (large files fail without ranged requests); write to an `impl std::io::Write`/`tokio` writer supplied by the caller.

Flow: URL → extract video ID (regex/string parsing) → InnerTube `player` request → deserialize player response → build `Video` with `Format` list → caller picks a format by quality/itag → decipher stream URL if needed → chunked ranged download.
