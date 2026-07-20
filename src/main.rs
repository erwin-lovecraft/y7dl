//! Thin demo binary for the `y7dl` library.
//!
//! Usage:
//!   y7dl <url-or-id>                          list available formats
//!   y7dl <url-or-id> --itag <N> -o <file>     download a format by itag
//!   y7dl <url-or-id> --quality <Q> -o <file>  download by quality (e.g. 720p)
//!   y7dl search <query>                       search YouTube
//!   y7dl search <query> --limit 5             limit results
//!   y7dl search <query> --json                output as JSON

use std::process::ExitCode;

use y7dl::{Client, Format, Video};

enum Command {
    Search { query: String, limit: usize, json: bool },
    Video(VideoArgs),
}

struct VideoArgs {
    url: String,
    itag: Option<u32>,
    quality: Option<String>,
    output: Option<String>,
}

fn parse_args() -> Option<Command> {
    let mut args = std::env::args().skip(1);
    let first = args.next()?;

    match first.as_str() {
        "search" => {
            let query = args.next()?;
            let mut limit = 10;
            let mut json = false;
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--limit" => limit = args.next()?.parse().ok()?,
                    "--json" => json = true,
                    _ => return None,
                }
            }
            Some(Command::Search { query, limit, json })
        }
        url => {
            let mut parsed = VideoArgs {
                url: url.to_string(),
                itag: None,
                quality: None,
                output: None,
            };
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--itag" => parsed.itag = Some(args.next()?.parse().ok()?),
                    "--quality" => parsed.quality = Some(args.next()?),
                    "-o" | "--output" => parsed.output = Some(args.next()?),
                    _ => return None,
                }
            }
            Some(Command::Video(parsed))
        }
    }
}

fn print_usage() {
    eprintln!("usage: y7dl <url-or-id>                          list available formats");
    eprintln!("       y7dl <url-or-id> --itag <N> -o <file>     download a format by itag");
    eprintln!("       y7dl <url-or-id> --quality <Q> -o <file>  download by quality (e.g. 720p)");
    eprintln!("       y7dl search <query>                       search YouTube");
    eprintln!("       y7dl search <query> --limit 5             limit results");
    eprintln!("       y7dl search <query> --json                output as JSON");
}

fn print_video(video: &Video) {
    println!(
        "{} — {} ({}s)",
        video.title,
        video.author,
        video.duration.as_secs()
    );
    println!(
        "{:>6}  {:<10} {:<12} {:>12}  mimeType",
        "itag", "quality", "audio", "bytes"
    );
    for f in &video.formats {
        println!(
            "{:>6}  {:<10} {:<12} {:>12}  {}",
            f.itag,
            f.quality_label
                .as_deref()
                .or(f.quality.as_deref())
                .unwrap_or("-"),
            if f.has_audio() { "yes" } else { "no" },
            f.content_length()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into()),
            f.mime_type,
        );
    }
}

fn pick_format<'a>(video: &'a Video, args: &VideoArgs) -> Option<&'a Format> {
    if let Some(itag) = args.itag {
        return video.format_by_itag(itag);
    }
    if let Some(quality) = &args.quality {
        return video.formats_with_quality(quality).into_iter().next();
    }
    None
}

async fn run_search(query: &str, limit: usize, json: bool) -> y7dl::Result<bool> {
    let client = Client::new();
    let results = client.search(query, limit, None).await?;

    if results.is_empty() {
        eprintln!("no results for {query:?}");
        return Ok(false);
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        println!("search results for {query:?}:\n");
        for (i, r) in results.iter().enumerate() {
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
    Ok(true)
}

async fn run_video(args: VideoArgs) -> y7dl::Result<bool> {
    let client = Client::new();
    let video = client.get_video(&args.url).await?;

    if args.itag.is_none() && args.quality.is_none() {
        print_video(&video);
        return Ok(true);
    }

    let Some(format) = pick_format(&video, &args) else {
        eprintln!("no matching format; available formats:");
        print_video(&video);
        return Ok(false);
    };
    let Some(output) = &args.output else {
        eprintln!("missing -o <file>");
        return Ok(false);
    };

    println!(
        "downloading itag {} ({}) to {output}...",
        format.itag, format.mime_type
    );
    let written = client.download_to_file(&video, format, output).await?;
    println!("done: {written} bytes");
    Ok(true)
}

#[tokio::main]
async fn main() -> ExitCode {
    let Some(cmd) = parse_args() else {
        print_usage();
        return ExitCode::FAILURE;
    };
    let result = match cmd {
        Command::Search { query, limit, json } => run_search(&query, limit, json).await,
        Command::Video(args) => run_video(args).await,
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
