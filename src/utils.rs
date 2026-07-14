//! Hand-rolled URL helpers (per project policy: no `url`/`regex` crates).

use crate::error::{Error, Result};

/// Extracts an 11-character video ID from a YouTube URL or returns the input
/// unchanged when it already is a bare ID.
///
/// Supported forms:
/// - `dQw4w9WgXcQ` (bare ID)
/// - `https://www.youtube.com/watch?v=dQw4w9WgXcQ`
/// - `https://youtu.be/dQw4w9WgXcQ`
/// - `https://www.youtube.com/embed/dQw4w9WgXcQ`
/// - `https://www.youtube.com/shorts/dQw4w9WgXcQ`
/// - `https://www.youtube.com/v/dQw4w9WgXcQ`
/// - `https://www.youtube.com/live/dQw4w9WgXcQ`
pub fn extract_video_id(input: &str) -> Result<String> {
    let input = input.trim();
    if is_video_id(input) {
        return Ok(input.to_owned());
    }

    let invalid = || Error::InvalidVideoId(input.to_owned());

    // Strip scheme and leading www./m./music. host prefixes.
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(input);
    let rest = rest
        .strip_prefix("www.")
        .or_else(|| rest.strip_prefix("m."))
        .or_else(|| rest.strip_prefix("music."))
        .unwrap_or(rest);

    let (host, path_and_query) = match rest.split_once('/') {
        Some((h, p)) => (h, p),
        None => return Err(invalid()),
    };

    let candidate = match host {
        "youtu.be" => first_path_segment(path_and_query),
        "youtube.com" | "youtube-nocookie.com" => {
            let (path, query) = match path_and_query.split_once('?') {
                Some((p, q)) => (p, Some(q)),
                None => (path_and_query, None),
            };
            match path.split_once('/') {
                // /embed/ID, /shorts/ID, /v/ID, /live/ID
                Some(("embed" | "shorts" | "v" | "live", tail)) => first_path_segment(tail),
                _ => match query {
                    // /watch?v=ID (and any other path carrying ?v=)
                    Some(query) => parse_query(query)
                        .into_iter()
                        .find(|(k, _)| k == "v")
                        .map(|(_, v)| v),
                    None => None,
                },
            }
        }
        _ => return Err(invalid()),
    };

    match candidate {
        Some(id) if is_video_id(&id) => Ok(id),
        _ => Err(invalid()),
    }
}

fn first_path_segment(path: &str) -> Option<String> {
    let end = path.find(['/', '?', '&', '#']).unwrap_or(path.len());
    if end == 0 {
        None
    } else {
        Some(path[..end].to_owned())
    }
}

fn is_video_id(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Parses an `application/x-www-form-urlencoded` query string into
/// percent-decoded key/value pairs, preserving order.
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

/// Decodes `%XX` escapes and `+` (as space). Invalid escapes pass through
/// verbatim. Decoded bytes are assumed to be UTF-8; invalid sequences are
/// replaced with U+FFFD.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_id_from_all_supported_forms() {
        let cases = [
            "dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "http://youtube.com/watch?v=dQw4w9WgXcQ&t=42s",
            "https://m.youtube.com/watch?feature=share&v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?t=10",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/v/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ?feature=share",
            "www.youtube.com/watch?v=dQw4w9WgXcQ",
        ];
        for case in cases {
            assert_eq!(
                extract_video_id(case).unwrap(),
                "dQw4w9WgXcQ",
                "failed for {case}"
            );
        }
    }

    #[test]
    fn rejects_invalid_input() {
        let cases = [
            "",
            "not an id",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?x=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=tooShort",
            "dQw4w9WgXc!",
        ];
        for case in cases {
            assert!(
                matches!(extract_video_id(case), Err(Error::InvalidVideoId(_))),
                "expected failure for {case}"
            );
        }
    }

    #[test]
    fn parses_and_decodes_query_strings() {
        let pairs = parse_query("s=ab%3Dcd&sp=sig&url=https%3A%2F%2Fexample.com%2F?a%2Bb");
        assert_eq!(
            pairs,
            vec![
                ("s".into(), "ab=cd".into()),
                ("sp".into(), "sig".into()),
                ("url".into(), "https://example.com/?a+b".into()),
            ]
        );
    }

    #[test]
    fn percent_decode_edge_cases() {
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode(""), "");
    }
}
