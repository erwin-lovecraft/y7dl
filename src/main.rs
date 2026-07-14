//! Thin demo binary for the `y7dl` library.
//!
//! Usage:
//!   y7dl <url-or-id>                          list available formats
//!   y7dl <url-or-id> --itag <N> -o <file>     download a format by itag
//!   y7dl <url-or-id> --quality <Q> -o <file>  download by quality (e.g. 720p)

use std::process::ExitCode;

use y7dl::{Client, Format, Video};

struct Args {
    url: String,
    itag: Option<u32>,
    quality: Option<String>,
    output: Option<String>,
}

fn parse_args() -> Option<Args> {
    let mut args = std::env::args().skip(1);
    let url = args.next()?;
    let mut parsed = Args {
        url,
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
    Some(parsed)
}

fn print_usage() {
    eprintln!("usage: y7dl <url-or-id>                          list available formats");
    eprintln!("       y7dl <url-or-id> --itag <N> -o <file>     download a format by itag");
    eprintln!("       y7dl <url-or-id> --quality <Q> -o <file>  download by quality (e.g. 720p)");
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

fn pick_format<'a>(video: &'a Video, args: &Args) -> Option<&'a Format> {
    if let Some(itag) = args.itag {
        return video.format_by_itag(itag);
    }
    if let Some(quality) = &args.quality {
        return video.formats_with_quality(quality).into_iter().next();
    }
    None
}

async fn run(args: Args) -> y7dl::Result<bool> {
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
    let Some(args) = parse_args() else {
        print_usage();
        return ExitCode::FAILURE;
    };
    match run(args).await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
