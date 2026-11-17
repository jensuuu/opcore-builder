use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("opcore-builder")
        .build()?)
}

pub fn fetch(url: &str) -> Result<Vec<u8>> {
    let response = client()?.get(url).send()?.error_for_status()?;
    Ok(response.bytes()?.to_vec())
}

fn api_asset(repo: &str, prefix: &str, contains: &str) -> Result<String> {
    let mut request = client()?
        .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .header("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        request = request.bearer_auth(token);
    }
    let release: Value = request.send()?.error_for_status()?.json()?;
    let prefix = prefix.to_lowercase();
    let contains = contains.to_lowercase();
    release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|asset| {
            let name = asset["name"].as_str()?.to_lowercase();
            if name.starts_with(&prefix)
                && name.contains(&contains)
                && name.ends_with(".zip")
                && !name.contains("debug")
            {
                asset["browser_download_url"].as_str().map(String::from)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("no matching release asset in {repo}"))
}

fn matches(name: &str, prefix: &str, contains: &str) -> bool {
    let name = name.to_lowercase();
    name.starts_with(&prefix.to_lowercase())
        && name.contains(&contains.to_lowercase())
        && name.ends_with(".zip")
        && !name.contains("debug")
}

/// Uses github.com web pages (no API, so no 60 requests/hour limit).
fn web_asset(repo: &str, prefix: &str, contains: &str) -> Result<String> {
    let client = client()?;
    let latest = client
        .get(format!("https://github.com/{repo}/releases/latest"))
        .send()?
        .error_for_status()?;
    let tag = latest.url().path().rsplit('/').next().unwrap_or("").to_string();
    if tag.is_empty() || tag == "latest" {
        bail!("no release found for {repo}");
    }
    let html = client
        .get(format!("https://github.com/{repo}/releases/expanded_assets/{tag}"))
        .send()?
        .error_for_status()?
        .text()?;
    // repo casing in links can differ from ours, so search case-insensitively
    let marker = format!("/{repo}/releases/download/{tag}/");
    let lower = html.to_ascii_lowercase();
    for (index, _) in lower.match_indices(&marker.to_ascii_lowercase()) {
        let start = index + marker.len();
        let Some(name) = html.get(start..).and_then(|rest| rest.split('"').next()) else { continue };
        if matches(name, prefix, contains) {
            return Ok(format!("https://github.com{}{name}", &html[index..start]));
        }
    }
    Err(anyhow!("no matching release asset in {repo}"))
}

pub fn release_asset(repo: &str, prefix: &str, contains: &str) -> Result<String> {
    if std::env::var("GITHUB_TOKEN").is_ok() {
        if let Ok(url) = api_asset(repo, prefix, contains) {
            return Ok(url);
        }
    }
    web_asset(repo, prefix, contains).or_else(|web| {
        api_asset(repo, prefix, contains).map_err(|api| anyhow!("{web}; API fallback: {api}"))
    })
}

pub fn extract_zip(url: &str, destination: &Path) -> Result<()> {
    let bytes = fetch(url)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    archive.extract(destination)?;
    Ok(())
}

/// Find a file or directory by name anywhere below `root` (zip layouts change between releases).
pub fn find(root: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find(&path, name) {
                return Some(found);
            }
        }
    }
    None
}
