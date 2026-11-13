use anyhow::{anyhow, Result};
use serde_json::Value;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("opcore-simplify")
        .build()?)
}

pub fn fetch(url: &str) -> Result<Vec<u8>> {
    let response = client()?.get(url).send()?.error_for_status()?;
    Ok(response.bytes()?.to_vec())
}

pub fn release_asset(repo: &str, prefix: &str, contains: &str) -> Result<String> {
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
