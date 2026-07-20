//! Network-dependent tests against the live YouTube API. Excluded from normal
//! runs (`#[ignore]`); run explicitly with `cargo test -- --ignored`.

use y7dl::Client;

const TEST_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

#[tokio::test]
#[ignore = "hits the live YouTube API"]
async fn fetches_video_info() {
    let client = Client::new();
    let video = client.get_video(TEST_URL).await.expect("get_video");

    assert_eq!(video.id, "dQw4w9WgXcQ");
    assert!(!video.title.is_empty(), "title should not be empty");
    assert!(!video.formats.is_empty(), "expected at least one format");

    let best = video.best_video().expect("a video format");
    assert!(best.itag > 0);
    println!(
        "OK: {} by {} — {} formats, best video: itag {} {} {}",
        video.title,
        video.author,
        video.formats.len(),
        best.itag,
        best.quality_label.as_deref().unwrap_or("?"),
        best.mime_type
    );
}

#[tokio::test]
#[ignore = "hits the live YouTube API and downloads data"]
async fn downloads_smallest_audio_stream() {
    let client = Client::new();
    let video = client.get_video(TEST_URL).await.expect("get_video");

    // Smallest audio-only format keeps the test fast.
    let format = video
        .audio_formats()
        .into_iter()
        .min_by_key(|f| f.content_length().unwrap_or(u64::MAX))
        .expect("an audio format");
    let expected = format.content_length().expect("content length");

    let path = std::env::temp_dir().join("y7dl_live_test.m4a");
    let written = client
        .download_to_file(&video, format, &path)
        .await
        .expect("download");
    let on_disk = std::fs::metadata(&path).expect("metadata").len();
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        written, expected,
        "bytes written should match contentLength"
    );
    assert_eq!(on_disk, expected, "file size should match contentLength");
    println!(
        "OK: downloaded itag {} ({}) — {} bytes",
        format.itag, format.mime_type, written
    );
}

#[tokio::test]
#[ignore = "hits the live YouTube API"]
async fn unavailable_video_yields_error() {
    let client = Client::new();
    // A deleted/invalid video ID.
    let err = client
        .get_video("aaaaaaaaaaa")
        .await
        .expect_err("expected failure");
    println!("OK, got expected error: {err}");
}

#[tokio::test]
#[ignore = "hits the live YouTube site"]
async fn search_returns_results() {
    let client = Client::new();
    let results = client
        .search("rust programming language", 5, None)
        .await
        .expect("search");

    assert!(!results.is_empty(), "expected at least one result");
    assert!(results.len() <= 5, "should respect the limit");

    println!("search results:");
    for (i, r) in results.iter().enumerate() {
        assert!(!r.video_id.is_empty(), "video_id should not be empty");
        assert!(!r.title.is_empty(), "title should not be empty");
        assert!(
            r.url.starts_with("https://www.youtube.com/watch?v="),
            "url should be a valid YouTube watch link"
        );
        println!(
            "  {}. {} — {} ({}) [{}]",
            i + 1,
            r.title,
            r.author,
            r.duration,
            r.url
        );
    }
}
