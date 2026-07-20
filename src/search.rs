//! YouTube search via the public `/results` page (no API key required).

use serde::Serialize;
use serde_json::Value;

use crate::error::Result;

const SEARCH_ENDPOINT: &str = "https://www.youtube.com/results";
/// Videos-only filter (`sp=EgIQAQ%3D%3D`).
const FILTER_VIDEOS: &str = "EgIQAQ%3D%3D";
/// Upload date: last hour.
pub const FILTER_LAST_HOUR: &str = "EgIIAQ%3D%3D";
/// Upload date: today.
pub const FILTER_TODAY: &str = "EgIIAw%3D%3D";
/// Upload date: this week.
pub const FILTER_THIS_WEEK: &str = "EgIIBA%3D%3D";
/// Upload date: this month.
pub const FILTER_THIS_MONTH: &str = "EgIIBQ%3D%3D";
/// Upload date: this year.
pub const FILTER_THIS_YEAR: &str = "EgIIBg%3D%3D";
/// Type: video.
pub const FILTER_TYPE_VIDEO: &str = "EgIQAQ%3D%3D";
/// Type: channel.
pub const FILTER_TYPE_CHANNEL: &str = "EgIQAg%3D%3D";
/// Type: playlist.
pub const FILTER_TYPE_PLAYLIST: &str = "EgIQAw%3D%3D";

/// A single YouTube search result.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub video_id: String,
    pub title: String,
    pub author: String,
    pub duration: String,
    pub views: String,
    pub url: String,
}

/// Recursively collect all `videoRenderer` entries from the nested JSON.
fn collect_video_renderers(value: &Value, results: &mut Vec<SearchResult>) {
    match value {
        Value::Object(obj) => {
            if let Some(vr) = obj.get("videoRenderer") {
                let video_id = vr
                    .get("videoId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if video_id.is_empty() {
                    return;
                }
                let title = vr
                    .pointer("/title/runs/0/text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let author = vr
                    .pointer("/longBylineText/runs/0/text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let duration = vr
                    .pointer("/lengthText/simpleText")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let views = vr
                    .pointer("/viewCountText/simpleText")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                results.push(SearchResult {
                    video_id: video_id.clone(),
                    title,
                    author,
                    duration,
                    views,
                    url: format!("https://www.youtube.com/watch?v={video_id}"),
                });
            }
            for v in obj.values() {
                collect_video_renderers(v, results);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_video_renderers(v, results);
            }
        }
        _ => {}
    }
}

/// Extract the `var ytInitialData = {...};` JSON string from the HTML.
fn extract_initial_data(html: &str) -> Option<&str> {
    let marker = "var ytInitialData = ";
    let start = html.find(marker)?;
    let json_start = start + marker.len();
    let bytes = html.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let mut started = false;
    for (i, &b) in bytes[json_start..].iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if b == b'\\' && in_str {
            escape = true;
            continue;
        }
        if b == b'"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        if b == b'{' {
            depth += 1;
            started = true;
        } else if b == b'}' {
            depth -= 1;
            if started && depth == 0 {
                return Some(&html[json_start..=json_start + i]);
            }
        }
    }
    None
}

/// Search YouTube for videos matching `query`.
///
/// `limit` caps the number of results returned (0 means no cap).
/// `filter` is an optional `sp` parameter for narrowing results
/// (e.g. [`FILTER_TODAY`], [`FILTER_TYPE_CHANNEL`]).
///
/// # Example
///
/// ```no_run
/// # async fn example() -> y7dl::Result<()> {
/// let client = y7dl::Client::new();
/// let results = client.search("rust programming", 5, None).await?;
/// for r in &results {
///     println!("{} - {} ({})", r.title, r.author, r.url);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn search(
    http: &reqwest::Client,
    query: &str,
    limit: usize,
    filter: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let mut req = http
        .get(SEARCH_ENDPOINT)
        .query(&[
            ("search_query", query),
            ("hl", "en"),
            ("gl", "US"),
        ]);
    if let Some(sp) = filter {
        req = req.query(&[("sp", sp)]);
    } else {
        req = req.query(&[("sp", FILTER_VIDEOS)]);
    }

    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(crate::error::Error::UnexpectedStatus(status.as_u16()));
    }
    let html = resp.text().await?;

    let json_str =
        extract_initial_data(&html).ok_or_else(|| crate::error::Error::Cipher(
            "failed to extract ytInitialData from search page (YouTube structure may have changed)"
                .into(),
        ))?;

    let data: Value = serde_json::from_str(json_str)?;
    let mut results = Vec::new();
    collect_video_renderers(&data, &mut results);
    if limit > 0 {
        results.truncate(limit);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_search_data() -> Value {
        json!({
            "contents": {
                "twoColumnSearchResultsRenderer": {
                    "primaryContents": {
                        "sectionListRenderer": {
                            "contents": [{
                                "itemSectionRenderer": {
                                    "contents": [{
                                        "videoRenderer": {
                                            "videoId": "abc123",
                                            "title": {"runs": [{"text": "Test Title"}]}
                                        }
                                    }]
                                }
                            }]
                        }
                    }
                }
            }
        })
    }

    fn sample_search_data_with_details() -> Value {
        json!({
            "contents": {
                "twoColumnSearchResultsRenderer": {
                    "primaryContents": {
                        "sectionListRenderer": {
                            "contents": [{
                                "itemSectionRenderer": {
                                    "contents": [{
                                        "videoRenderer": {
                                            "videoId": "dQw4w9WgXcQ",
                                            "title": {"runs": [{"text": "Never Gonna Give You Up"}]},
                                            "longBylineText": {"runs": [{"text": "Rick Astley"}]},
                                            "lengthText": {"simpleText": "3:33"},
                                            "viewCountText": {"simpleText": "1,234,567 views"}
                                        }
                                    }]
                                }
                            }]
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn extract_initial_data_finds_json() {
        let data = sample_search_data();
        let html = format!(
            r#"<!DOCTYPE html><script>var ytInitialData = {};</script>"#,
            data
        );
        let extracted = extract_initial_data(&html).expect("should find ytInitialData");
        let parsed: Value = serde_json::from_str(extracted).unwrap();
        let id = parsed
            .pointer("/contents/twoColumnSearchResultsRenderer/primaryContents/sectionListRenderer/contents/0/itemSectionRenderer/contents/0/videoRenderer/videoId")
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(id, "abc123");
    }

    #[test]
    fn collect_video_renderers_extracts_results() {
        let data = sample_search_data_with_details();
        let mut results = Vec::new();
        collect_video_renderers(&data, &mut results);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].video_id, "dQw4w9WgXcQ");
        assert_eq!(results[0].title, "Never Gonna Give You Up");
        assert_eq!(results[0].author, "Rick Astley");
        assert_eq!(results[0].duration, "3:33");
        assert_eq!(results[0].views, "1,234,567 views");
        assert_eq!(
            results[0].url,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live YouTube site"]
    async fn live_search_returns_results() {
        let client = reqwest::Client::new();
        let results = search(&client, "rust programming", 5, None)
            .await
            .unwrap();
        assert!(!results.is_empty());
        for r in &results {
            assert!(!r.video_id.is_empty());
            assert!(!r.title.is_empty());
            println!("{} - {} ({})", r.title, r.author, r.url);
        }
    }
}
