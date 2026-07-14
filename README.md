# y7dl

A Rust port of [kkdai/youtube](https://github.com/kkdai/youtube): a library for
extracting YouTube video info (itag, quality, mime type) from a link and
downloading a stream at a chosen quality.

> This project exists for learning purposes and is fully open source under the
> [GPL-3.0-or-later](LICENSE) license. Its copyleft terms mean that any project
> incorporating this code must be open source as well — see
> [License](#license).

**Scope:** info extraction and stream download only. Transcoding/muxing,
playlists, and captions are intentionally out of scope. Adaptive (DASH)
formats are downloaded as-is — video-only and audio-only streams are separate
files; merging them is up to you (e.g. with ffmpeg).

## Features

- Extract video metadata (title, author, duration, view count) and the full
  format list — itag, quality/qualityLabel, mimeType, bitrate, size — from a
  URL or bare video ID.
- Pick a format by itag, by quality (`720p`, `hd720`, ...), or via
  `best_video()` / `best_audio()` helpers.
- Download streams with chunked HTTP `Range` requests (10 MB chunks) into any
  `tokio::io::AsyncWrite`, or straight to a file.
- Minimal dependencies: `tokio`, `serde`, `serde_json`, `reqwest`. No JS
  engine, no regex crate.
- Errors follow Rust conventions: one crate-wide `Error` enum returned through
  `y7dl::Result<T>`; no panics on bad input or API surprises.

## Library usage

```rust
use y7dl::{Client, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::new();

    // Accepts watch/youtu.be/embed/shorts URLs or a bare 11-char ID.
    let video = client
        .get_video("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
        .await?;

    println!("{} by {} ({}s)", video.title, video.author, video.duration.as_secs());

    // Inspect available formats.
    for format in &video.formats {
        println!(
            "itag {:>3}  {:<8}  {}",
            format.itag,
            format.quality_label.as_deref().unwrap_or("-"),
            format.mime_type,
        );
    }

    // Pick a format: by itag, by quality, or best available.
    let format = video
        .formats_with_quality("720p")
        .into_iter()
        .next()
        .or_else(|| video.best_video())
        .expect("no formats");

    // Download it.
    let bytes = client.download_to_file(&video, format, "video.mp4").await?;
    println!("downloaded {bytes} bytes");
    Ok(())
}
```

Useful `Video` helpers:

| Method | Returns |
|---|---|
| `format_by_itag(18)` | the format with that itag, if any |
| `formats_with_quality("720p")` | formats matching `quality` or `qualityLabel` |
| `video_formats()` / `audio_formats()` | formats by mime-type class |
| `best_video()` / `best_audio()` | highest resolution / highest bitrate |

For custom proxies or timeouts, build your own `reqwest::Client` and pass it
to `Client::with_http_client`. Reuse one `Client` across calls — it pools
connections and caches parsed player data.

### Error handling

Every fallible API returns `y7dl::Result<T>`. Match on the enum to
distinguish failure classes:

```rust
use y7dl::Error;

match client.get_video(url).await {
    Ok(video) => { /* ... */ }
    Err(Error::InvalidVideoId(input)) => eprintln!("not a YouTube link: {input}"),
    Err(Error::VideoUnavailable { status, reason }) => {
        eprintln!("unplayable ({status}): {}", reason.unwrap_or_default())
    }
    Err(Error::Http(e)) => eprintln!("network problem: {e}"),
    Err(e) => eprintln!("{e}"),
}
```

## CLI demo

A thin demo binary ships with the crate:

```console
$ cargo run -- https://www.youtube.com/watch?v=dQw4w9WgXcQ
Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster) — Rick Astley (213s)
  itag  quality    audio               bytes  mimeType
    18  360p       yes                     -  video/mp4; codecs="avc1.42001E, mp4a.40.2"
   313  2160p      no              358608461  video/webm; codecs="vp9"
   ...

$ cargo run -- dQw4w9WgXcQ --itag 18 -o video.mp4
downloading itag 18 (video/mp4; codecs="avc1.42001E, mp4a.40.2") to video.mp4...
done: 11829048 bytes

$ cargo run -- dQw4w9WgXcQ --quality 720p -o video.mp4
```

## How it works

1. The video ID is parsed out of the URL.
2. Video metadata is fetched from YouTube's InnerTube API
   (`/youtubei/v1/player`), impersonating the `ANDROID_VR` client. Unlike the
   `ANDROID`/`IOS` clients (rejected by YouTube since late 2024 without
   attestation tokens), `ANDROID_VR` still returns direct, un-ciphered stream
   URLs.
3. The chosen format's stream is downloaded in 10 MB ranged chunks.

For formats that carry a `signatureCipher` instead of a direct URL (never the
case on the default path), the crate falls back to fetching the player's
`base.js` and extracting the classic reverse/splice/swap signature transforms
with string parsing. Modern (2025+) players hide this logic behind
runtime-decoded JS that would require a JavaScript interpreter (the Go
original embeds [goja](https://github.com/dop251/goja) for this) — that is out
of scope here, and such formats return `Error::Cipher`. The `n`-parameter
throttle transform is likewise not ported; it is not needed on the default
path.

## Development

```console
cargo build
cargo test               # offline tests only (fixtures)
cargo test -- --ignored  # live tests against the real YouTube API
cargo clippy
cargo fmt
```

Live tests are `#[ignore]`d because they depend on the network and on
YouTube's behavior; the parsing logic is covered offline by fixture JSON in
`tests/fixtures/`.

## License

Licensed under the [GNU General Public License v3.0 or later](LICENSE)
(`GPL-3.0-or-later`).

This is a strong copyleft license: you are free to use, study, modify, and
redistribute this code, but **any project that incorporates it — including
projects that link against it as a library — must itself be released as open
source under GPL-compatible terms**, with its source code made available to
its users. If your project cannot meet that requirement, do not use this code.

This project was written for learning — study it, fork it, break it, port it.

## Disclaimer

This is an unofficial library using YouTube's internal APIs, which change
without notice. Downloading videos may violate YouTube's Terms of Service;
use it only for content you have the right to download.
