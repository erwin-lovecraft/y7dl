use std::fs;

use y7dl::Client;

#[tokio::main]
async fn main() -> y7dl::Result<()> {
    let client = Client::new();
    let results = client.search("rust", 5, None).await?;
    let json = serde_json::to_string_pretty(&results)?;
    let path = "tests/fixtures/search_results.json";
    fs::write(path, &json)?;
    println!("saved {} results to {path}", results.len());
    println!("{json}");
    Ok(())
}
