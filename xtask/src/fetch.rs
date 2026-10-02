use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client, Response};

const CDN: &str = "https://cdn.jsdelivr.net/gh";
const API: &str = "https://api.github.com";
const ATTEMPTS: u32 = 3;

pub fn document(repo: &str, rev: &str, path: &str) -> Result<Vec<u8>> {
    let url = format!("{CDN}/{repo}@{rev}/{path}");
    let response = attempt(&url, |client| client.get(&url).send())?;
    let status = response.status();
    if !status.is_success() {
        bail!("{url} answered {status}");
    }
    let bytes = response
        .bytes()
        .with_context(|| format!("reading {url}"))?
        .to_vec();
    if bytes.is_empty() {
        bail!("{url} answered an empty body");
    }
    Ok(bytes)
}

pub fn head(repo: &str) -> Result<String> {
    let url = format!("{API}/repos/{repo}/commits/HEAD");
    let response = attempt(&url, |client| {
        client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
    })?;
    let status = response.status();
    if !status.is_success() {
        bail!("{url} answered {status}");
    }
    let bytes = response.bytes().with_context(|| format!("reading {url}"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).with_context(|| format!("{url} is not json"))?;
    value
        .get("sha")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("{url} carries no sha"))
}

fn attempt(url: &str, send: impl Fn(&Client) -> reqwest::Result<Response>) -> Result<Response> {
    let client = client()?;
    let mut last = None;
    for round in 0..ATTEMPTS {
        if round > 0 {
            sleep(Duration::from_secs(u64::from(round)));
        }
        match send(&client) {
            Ok(response) => return Ok(response),
            Err(error) => last = Some(error),
        }
    }
    let error = last.context("no attempt ran")?;
    Err(error).with_context(|| format!("fetching {url}"))
}

fn client() -> Result<Client> {
    Client::builder()
        .user_agent(concat!("awitch-xtask/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(300))
        .build()
        .context("building the http client")
}
