//! Public data model: a [`Video`] and its available [`Format`]s.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::response::PlayerResponse;

/// A single downloadable stream variant (one entry of `streamingData.formats`
/// or `streamingData.adaptiveFormats`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    pub itag: u32,
    /// Direct stream URL. Absent when the stream is protected by
    /// `signature_cipher` and must be deciphered first.
    pub url: Option<String>,
    /// e.g. `video/mp4; codecs="avc1.42001E, mp4a.40.2"`.
    pub mime_type: String,
    pub bitrate: Option<u64>,
    pub average_bitrate: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    /// Total stream size in bytes, sent by the API as a string.
    pub content_length: Option<String>,
    /// Coarse quality bucket, e.g. `tiny`, `medium`, `hd720`.
    pub quality: Option<String>,
    /// Human label for video formats, e.g. `720p`, `1080p60`.
    pub quality_label: Option<String>,
    /// e.g. `AUDIO_QUALITY_MEDIUM`; present on audio-carrying formats.
    pub audio_quality: Option<String>,
    pub audio_sample_rate: Option<String>,
    pub audio_channels: Option<u32>,
    pub approx_duration_ms: Option<String>,
    /// `s=...&sp=...&url=...` blob for cipher-protected streams.
    pub signature_cipher: Option<String>,
}

impl Format {
    /// True when the mime type is `video/*` (may also carry audio if progressive).
    pub fn is_video(&self) -> bool {
        self.mime_type.starts_with("video/")
    }

    /// True when the mime type is `audio/*`.
    pub fn is_audio(&self) -> bool {
        self.mime_type.starts_with("audio/")
    }

    /// True when the format carries an audio track (audio-only or progressive).
    pub fn has_audio(&self) -> bool {
        self.is_audio() || self.audio_quality.is_some() || self.audio_channels.is_some()
    }

    /// Total stream size in bytes, when the API reports it.
    pub fn content_length(&self) -> Option<u64> {
        self.content_length.as_deref()?.parse().ok()
    }

    /// True when this format matches a quality string: exact `quality`
    /// (e.g. `hd720`) or `qualityLabel` (e.g. `720p`) match.
    pub fn matches_quality(&self, quality: &str) -> bool {
        self.quality.as_deref() == Some(quality) || self.quality_label.as_deref() == Some(quality)
    }
}

/// Video metadata and the merged list of all available formats.
#[derive(Debug, Clone)]
pub struct Video {
    pub id: String,
    pub title: String,
    pub author: String,
    pub duration: Duration,
    pub description: String,
    pub view_count: u64,
    /// Progressive formats first (`streamingData.formats`), then adaptive ones.
    pub formats: Vec<Format>,
}

impl Video {
    /// Builds a `Video` from a raw player response, enforcing playability.
    pub(crate) fn from_player_response(id: &str, response: PlayerResponse) -> Result<Self> {
        let status = response
            .playability_status
            .as_ref()
            .and_then(|s| s.status.as_deref())
            .unwrap_or("UNKNOWN");
        if !status.eq_ignore_ascii_case("OK") {
            return Err(Error::VideoUnavailable {
                status: status.to_owned(),
                reason: response.playability_status.and_then(|s| s.reason),
            });
        }

        let details = response
            .video_details
            .ok_or(Error::MissingField("videoDetails"))?;
        let streaming = response
            .streaming_data
            .ok_or(Error::MissingField("streamingData"))?;

        let mut formats = streaming.formats;
        formats.extend(streaming.adaptive_formats);

        let seconds = details
            .length_seconds
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0u64);

        Ok(Video {
            id: details.video_id.unwrap_or_else(|| id.to_owned()),
            title: details.title.unwrap_or_default(),
            author: details.author.unwrap_or_default(),
            duration: Duration::from_secs(seconds),
            description: details.short_description.unwrap_or_default(),
            view_count: details
                .view_count
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            formats,
        })
    }

    /// The format with the given itag, if available.
    pub fn format_by_itag(&self, itag: u32) -> Option<&Format> {
        self.formats.iter().find(|f| f.itag == itag)
    }

    /// All formats matching a quality string (`hd720`, `720p`, ...).
    pub fn formats_with_quality(&self, quality: &str) -> Vec<&Format> {
        self.formats
            .iter()
            .filter(|f| f.matches_quality(quality))
            .collect()
    }

    /// All `video/*` formats.
    pub fn video_formats(&self) -> Vec<&Format> {
        self.formats.iter().filter(|f| f.is_video()).collect()
    }

    /// All `audio/*` formats.
    pub fn audio_formats(&self) -> Vec<&Format> {
        self.formats.iter().filter(|f| f.is_audio()).collect()
    }

    /// Highest-resolution video format (ties broken by bitrate).
    pub fn best_video(&self) -> Option<&Format> {
        self.formats
            .iter()
            .filter(|f| f.is_video())
            .max_by_key(|f| {
                (
                    f.height.unwrap_or(0),
                    f.fps.unwrap_or(0),
                    f.bitrate.unwrap_or(0),
                )
            })
    }

    /// Highest-bitrate audio-only format.
    pub fn best_audio(&self) -> Option<&Format> {
        self.formats
            .iter()
            .filter(|f| f.is_audio())
            .max_by_key(|f| f.bitrate.unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/player_response.json");

    fn video() -> Video {
        let response: PlayerResponse = serde_json::from_str(FIXTURE).unwrap();
        Video::from_player_response("dQw4w9WgXcQ", response).unwrap()
    }

    #[test]
    fn parses_fixture_metadata() {
        let v = video();
        assert_eq!(v.id, "dQw4w9WgXcQ");
        assert_eq!(v.title, "Test Video");
        assert_eq!(v.author, "Test Channel");
        assert_eq!(v.duration, Duration::from_secs(212));
        assert_eq!(v.view_count, 1234567);
        assert_eq!(v.formats.len(), 4);
    }

    #[test]
    fn format_filters_work() {
        let v = video();

        let f18 = v.format_by_itag(18).expect("itag 18");
        assert_eq!(f18.quality_label.as_deref(), Some("360p"));
        assert!(f18.is_video() && f18.has_audio());
        assert_eq!(f18.content_length(), Some(10_000_000));

        assert_eq!(v.formats_with_quality("720p").len(), 1);
        assert_eq!(v.formats_with_quality("hd720").len(), 1);
        assert_eq!(v.video_formats().len(), 3);
        assert_eq!(v.audio_formats().len(), 1);

        assert_eq!(v.best_video().unwrap().itag, 137);
        assert_eq!(v.best_audio().unwrap().itag, 140);
        assert!(v.format_by_itag(9999).is_none());
    }

    #[test]
    fn unplayable_video_maps_to_error() {
        let json = r#"{
            "playabilityStatus": {"status": "LOGIN_REQUIRED", "reason": "Sign in to confirm your age"}
        }"#;
        let response: PlayerResponse = serde_json::from_str(json).unwrap();
        let err = Video::from_player_response("dQw4w9WgXcQ", response).unwrap_err();
        match err {
            Error::VideoUnavailable { status, reason } => {
                assert_eq!(status, "LOGIN_REQUIRED");
                assert_eq!(reason.as_deref(), Some("Sign in to confirm your age"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
