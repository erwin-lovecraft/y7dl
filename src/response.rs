//! Serde structs mirroring the InnerTube `/youtubei/v1/player` response.
//! Only the fields this crate consumes are modeled; unknown fields are ignored.

use serde::Deserialize;

use crate::video::Format;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerResponse {
    pub response_context: Option<ResponseContext>,
    pub playability_status: Option<PlayabilityStatus>,
    pub video_details: Option<VideoDetails>,
    pub streaming_data: Option<StreamingData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseContext {
    /// Anonymous session token. Echoing it back defeats the per-video
    /// "Sign in to confirm you're not a bot" check.
    pub visitor_data: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayabilityStatus {
    pub status: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDetails {
    pub video_id: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    /// Duration in seconds, sent by the API as a string.
    pub length_seconds: Option<String>,
    pub short_description: Option<String>,
    pub view_count: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingData {
    #[serde(default)]
    pub formats: Vec<Format>,
    #[serde(default)]
    pub adaptive_formats: Vec<Format>,
}
