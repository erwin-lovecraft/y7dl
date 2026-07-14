use std::fmt;

/// Crate-wide result alias returned by all fallible public APIs.
pub type Result<T> = std::result::Result<T, Error>;

/// All failure classes of this crate.
#[derive(Debug)]
pub enum Error {
    /// The input could not be parsed into an 11-character YouTube video ID.
    InvalidVideoId(String),
    /// Network or HTTP-level failure.
    Http(reqwest::Error),
    /// The API returned a non-success HTTP status.
    UnexpectedStatus(u16),
    /// The API response could not be deserialized.
    Json(serde_json::Error),
    /// Local I/O failure while writing a downloaded stream.
    Io(std::io::Error),
    /// YouTube reported the video as not playable (`playabilityStatus` != OK),
    /// e.g. private, removed, region-locked, or age-restricted.
    VideoUnavailable {
        status: String,
        reason: Option<String>,
    },
    /// No format matched the requested itag/quality filter.
    FormatNotFound,
    /// A stream URL could not be deciphered from `signatureCipher`.
    Cipher(String),
    /// A required field was missing from the API response.
    MissingField(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidVideoId(input) => {
                write!(f, "could not extract a video ID from {input:?}")
            }
            Error::Http(e) => write!(f, "http request failed: {e}"),
            Error::UnexpectedStatus(code) => write!(f, "unexpected http status {code}"),
            Error::Json(e) => write!(f, "failed to parse api response: {e}"),
            Error::Io(e) => write!(f, "i/o error: {e}"),
            Error::VideoUnavailable { status, reason } => match reason {
                Some(reason) => write!(f, "video unavailable ({status}): {reason}"),
                None => write!(f, "video unavailable ({status})"),
            },
            Error::FormatNotFound => write!(f, "no format matched the requested filter"),
            Error::Cipher(msg) => write!(f, "failed to decipher stream url: {msg}"),
            Error::MissingField(field) => {
                write!(f, "api response is missing required field `{field}`")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Http(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Http(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
