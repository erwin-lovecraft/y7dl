//! The public entry point: fetches video metadata via YouTube's InnerTube API.

use serde_json::json;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::decipher::{self, SigOp};
use crate::error::{Error, Result};
use crate::response::PlayerResponse;
use crate::utils::{extract_video_id, parse_query};
use crate::video::{Format, Video};

const PLAYER_ENDPOINT: &str = "https://www.youtube.com/youtubei/v1/player";

/// Streams are fetched in ranged chunks: googlevideo throttles or drops
/// unbounded requests for large files (the Go library does the same).
const DOWNLOAD_CHUNK_SIZE: u64 = 10 * 1024 * 1024;

// The ANDROID_VR InnerTube client: its player responses carry direct stream
// URLs, avoiding signature ciphering entirely. The Go library defaults to the
// plain ANDROID client, but as of late 2024 InnerTube rejects ANDROID/IOS
// requests without attestation tokens (HTTP 400 FAILED_PRECONDITION), while
// ANDROID_VR is still accepted (verified 2026-07).
const VR_CLIENT_NAME: &str = "ANDROID_VR";
const VR_CLIENT_ID: &str = "28";
const VR_CLIENT_VERSION: &str = "1.60.19";
const VR_USER_AGENT: &str = "com.google.android.apps.youtube.vr.oculus/1.60.19 \
     (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip";

/// A YouTube API client. Cheap to clone the inner `reqwest::Client`; reuse one
/// `Client` across requests to benefit from connection pooling.
pub struct Client {
    http: reqwest::Client,
    /// Parsed signature ops cached per player-JS path (fetching and parsing
    /// `base.js` is expensive; the player version changes rarely).
    player_ops: Mutex<Option<(String, Vec<SigOp>)>>,
    /// Anonymous session token from previous responses. Some videos answer
    /// `LOGIN_REQUIRED: Sign in to confirm you're not a bot` unless the
    /// request carries the `visitorData` YouTube itself handed out.
    visitor_data: Mutex<Option<String>>,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        Self::with_http_client(reqwest::Client::new())
    }

    /// Uses a caller-provided `reqwest::Client` (proxies, timeouts, ...).
    pub fn with_http_client(http: reqwest::Client) -> Self {
        Client {
            http,
            player_ops: Mutex::new(None),
            visitor_data: Mutex::new(None),
        }
    }

    /// Fetches metadata and the available formats for a video URL or bare ID.
    pub async fn get_video(&self, url_or_id: &str) -> Result<Video> {
        let id = extract_video_id(url_or_id)?;

        let sent_visitor = self.visitor_data.lock().await.clone();
        let response = self.player_response(&id).await?;
        match Video::from_player_response(&id, response) {
            // Per-video bot check: even the first, refused response carries a
            // fresh `visitorData` token; echoing it back satisfies the check.
            Err(Error::VideoUnavailable { status, reason }) if status == "LOGIN_REQUIRED" => {
                let visitor_now = self.visitor_data.lock().await.clone();
                if visitor_now.is_some() && visitor_now != sent_visitor {
                    let retry = self.player_response(&id).await?;
                    Video::from_player_response(&id, retry)
                } else {
                    Err(Error::VideoUnavailable { status, reason })
                }
            }
            other => other,
        }
    }

    async fn player_response(&self, video_id: &str) -> Result<PlayerResponse> {
        let visitor_data = self.visitor_data.lock().await.clone();

        let mut client_context = json!({
            "clientName": VR_CLIENT_NAME,
            "clientVersion": VR_CLIENT_VERSION,
            "deviceMake": "Oculus",
            "deviceModel": "Quest 3",
            "androidSdkVersion": 32,
            "osName": "Android",
            "osVersion": "12L",
            "userAgent": VR_USER_AGENT,
            "hl": "en",
            "gl": "US",
            "utcOffsetMinutes": 0
        });
        if let Some(vd) = &visitor_data {
            client_context["visitorData"] = json!(vd);
        }
        let body = json!({
            "videoId": video_id,
            "context": { "client": client_context },
            "contentCheckOk": true,
            "racyCheckOk": true
        });

        let mut request = self
            .http
            .post(PLAYER_ENDPOINT)
            .query(&[("prettyPrint", "false")])
            .header("User-Agent", VR_USER_AGENT)
            .header("X-Youtube-Client-Name", VR_CLIENT_ID)
            .header("X-Youtube-Client-Version", VR_CLIENT_VERSION);
        if let Some(vd) = &visitor_data {
            request = request.header("X-Goog-Visitor-Id", vd.as_str());
        }
        let response = request.json(&body).send().await?;

        let status = response.status();
        if !status.is_success() {
            return Err(Error::UnexpectedStatus(status.as_u16()));
        }

        let text = response.text().await?;
        let parsed: PlayerResponse = serde_json::from_str(&text)?;

        // Remember the session token YouTube handed out for later requests.
        if let Some(vd) = parsed
            .response_context
            .as_ref()
            .and_then(|c| c.visitor_data.clone())
        {
            *self.visitor_data.lock().await = Some(vd);
        }
        Ok(parsed)
    }

    /// Resolves the playable stream URL for a format, deciphering
    /// `signatureCipher` when the format has no direct `url`.
    pub async fn stream_url(&self, video: &Video, format: &Format) -> Result<String> {
        if let Some(url) = &format.url {
            return Ok(url.clone());
        }
        let cipher = format
            .signature_cipher
            .as_deref()
            .ok_or(Error::MissingField("url or signatureCipher"))?;

        let mut s = None;
        let mut sp = None;
        let mut url = None;
        for (k, v) in parse_query(cipher) {
            match k.as_str() {
                "s" => s = Some(v),
                "sp" => sp = Some(v),
                "url" => url = Some(v),
                _ => {}
            }
        }
        let s = s.ok_or_else(|| Error::Cipher("signatureCipher has no `s`".into()))?;
        let url = url.ok_or_else(|| Error::Cipher("signatureCipher has no `url`".into()))?;
        let sp = sp.unwrap_or_else(|| "signature".to_owned());

        let ops = self.signature_ops(&video.id).await?;
        let sig = decipher::apply_ops(&ops, &s);
        Ok(format!("{url}&{sp}={sig}"))
    }

    /// Returns the signature ops for the current player, fetching and parsing
    /// `base.js` only when the player version changed since the last call.
    async fn signature_ops(&self, video_id: &str) -> Result<Vec<SigOp>> {
        let embed_html = self
            .get_text(&format!("https://www.youtube.com/embed/{video_id}"))
            .await?;
        let path = decipher::find_player_js_path(&embed_html)
            .ok_or_else(|| Error::Cipher("player js path not found in embed page".into()))?;

        let mut cache = self.player_ops.lock().await;
        if let Some((cached_path, ops)) = cache.as_ref()
            && *cached_path == path
        {
            return Ok(ops.clone());
        }

        let js = self
            .get_text(&format!("https://www.youtube.com{path}"))
            .await?;
        let ops = decipher::parse_signature_ops(&js)?;
        *cache = Some((path, ops.clone()));
        Ok(ops)
    }

    async fn get_text(&self, url: &str) -> Result<String> {
        let response = self.http.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::UnexpectedStatus(status.as_u16()));
        }
        Ok(response.text().await?)
    }

    /// Downloads a format's stream into `dest` using ranged chunk requests.
    /// Returns the number of bytes written.
    pub async fn download<W>(&self, video: &Video, format: &Format, dest: &mut W) -> Result<u64>
    where
        W: AsyncWrite + Unpin + ?Sized,
    {
        let url = self.stream_url(video, format).await?;

        let total = match format.content_length() {
            Some(len) => len,
            None => self.probe_content_length(&url).await?,
        };

        let mut written = 0u64;
        if total == 0 {
            // Unknown size: stream a single unbounded request.
            written += self.download_range(&url, None, dest).await?;
        } else {
            let mut start = 0u64;
            while start < total {
                let end = (start + DOWNLOAD_CHUNK_SIZE - 1).min(total - 1);
                written += self.download_range(&url, Some((start, end)), dest).await?;
                start = end + 1;
            }
        }
        dest.flush().await?;
        Ok(written)
    }

    /// Convenience wrapper: downloads a format to a file path.
    pub async fn download_to_file(
        &self,
        video: &Video,
        format: &Format,
        path: impl AsRef<std::path::Path>,
    ) -> Result<u64> {
        let mut file = tokio::fs::File::create(path).await?;
        self.download(video, format, &mut file).await
    }

    async fn download_range<W>(
        &self,
        url: &str,
        range: Option<(u64, u64)>,
        dest: &mut W,
    ) -> Result<u64>
    where
        W: AsyncWrite + Unpin + ?Sized,
    {
        let mut request = self.http.get(url).header("User-Agent", VR_USER_AGENT);
        if let Some((start, end)) = range {
            request = request.header("Range", format!("bytes={start}-{end}"));
        }
        let mut response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::UnexpectedStatus(status.as_u16()));
        }

        let mut written = 0u64;
        while let Some(chunk) = response.chunk().await? {
            dest.write_all(&chunk).await?;
            written += chunk.len() as u64;
        }
        Ok(written)
    }

    /// Asks the stream server for the total size when the API did not report it.
    async fn probe_content_length(&self, url: &str) -> Result<u64> {
        let response = self
            .http
            .head(url)
            .header("User-Agent", VR_USER_AGENT)
            .send()
            .await?;
        Ok(response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises player discovery (embed page → base.js) against the live
    /// site. Signature-op extraction only works for classic-format players;
    /// 2025+ players pack the cipher code in a runtime-decoded string table,
    /// which requires a JS interpreter (goja in the Go original) that is out
    /// of scope here — for those, a clean `Error::Cipher` is the contract.
    /// Run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "hits the live YouTube API"]
    async fn discovers_live_player_js() {
        let client = Client::new();
        match client.signature_ops("dQw4w9WgXcQ").await {
            Ok(ops) => {
                assert!(!ops.is_empty());
                println!("live player uses classic cipher; ops: {ops:?}");
            }
            Err(Error::Cipher(msg)) => {
                println!("live player is not classic-format (expected for 2025+ players): {msg}");
            }
            Err(other) => panic!("player discovery failed: {other}"),
        }
    }
}
