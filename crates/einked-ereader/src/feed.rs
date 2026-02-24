extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedType {
    Opds,
    Rss,
}

#[derive(Debug, Clone)]
pub struct FeedSource {
    pub name: String,
    pub url: String,
    pub feed_type: FeedType,
}

impl FeedSource {
    pub fn new(name: &str, url: &str, feed_type: FeedType) -> Self {
        Self {
            name: name.to_string(),
            url: url.to_string(),
            feed_type,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpdsCatalog {
    pub title: String,
    pub subtitle: Option<String>,
    pub entries: Vec<OpdsEntry>,
    pub links: Vec<OpdsLink>,
}

#[derive(Debug, Clone)]
pub struct OpdsEntry {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub cover_url: Option<String>,
    pub download_url: Option<String>,
    pub format: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct OpdsLink {
    pub href: String,
    pub rel: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FeedEntryData {
    pub title: String,
    pub url: Option<String>,
    pub summary: Option<String>,
}

pub trait FeedClient {
    fn fetch_entries(
        &mut self,
        source_name: &str,
        source_url: &str,
        source_type: FeedType,
    ) -> Result<Vec<FeedEntryData>, String>;

    fn fetch_article_lines(&mut self, url: &str) -> Result<Vec<String>, String>;
}

#[derive(Default)]
pub struct NoopFeedClient;

impl FeedClient for NoopFeedClient {
    fn fetch_entries(
        &mut self,
        _source_name: &str,
        _source_url: &str,
        _source_type: FeedType,
    ) -> Result<Vec<FeedEntryData>, String> {
        Err("Feed client is unavailable on this target.".to_string())
    }

    fn fetch_article_lines(&mut self, _url: &str) -> Result<Vec<String>, String> {
        Err("Article rendering is unavailable on this target.".to_string())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[derive(Default)]
pub struct HostFeedClient;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl FeedClient for HostFeedClient {
    fn fetch_entries(
        &mut self,
        _source_name: &str,
        source_url: &str,
        _source_type: FeedType,
    ) -> Result<Vec<FeedEntryData>, String> {
        let base_url = url::Url::parse(source_url).ok();
        let response = ureq::get(source_url)
            .call()
            .map_err(|_| "Failed to fetch feed.".to_string())?;
        let mut body = response.into_body();
        let bytes = body
            .read_to_vec()
            .map_err(|_| "Failed reading feed response.".to_string())?;
        let parsed =
            feed_rs::parser::parse(&bytes[..]).map_err(|_| "Failed to parse feed.".to_string())?;
        let mut entries = Vec::new();
        for entry in parsed.entries.iter().take(32) {
            entries.push(FeedEntryData {
                title: entry
                    .title
                    .as_ref()
                    .map(|t| t.content.clone())
                    .unwrap_or_else(|| "Untitled".to_string()),
                url: entry.links.first().map(|l| {
                    if let Some(base) = &base_url
                        && let Ok(joined) = base.join(&l.href)
                    {
                        return joined.to_string();
                    }
                    l.href.clone()
                }),
                summary: entry.summary.as_ref().map(|s| s.content.clone()),
            });
        }
        if entries.is_empty() {
            Err("Feed returned no entries.".to_string())
        } else {
            Ok(entries)
        }
    }

    fn fetch_article_lines(&mut self, url: &str) -> Result<Vec<String>, String> {
        let reader_url = get_reader_url(url);
        let response = ureq::get(&reader_url)
            .call()
            .map_err(|_| "Failed to fetch article.".to_string())?;
        let mut body = response.into_body();
        let bytes = body
            .read_to_vec()
            .map_err(|_| "Failed reading article body.".to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        let mut lines = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                lines.push(String::new());
            } else {
                lines.push(trimmed.to_string());
            }
        }
        if lines.is_empty() {
            Err("Article had no readable text.".to_string())
        } else {
            Ok(lines)
        }
    }
}

pub fn default_feed_client() -> Box<dyn FeedClient> {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        Box::new(HostFeedClient)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Box::new(NoopFeedClient)
    }
}

pub const PRELOADED_OPDS_SOURCES: &[(&str, &str)] = &[
    ("Project Gutenberg", "https://m.gutenberg.org/ebooks.opds/"),
    ("Standard Ebooks", "https://standardebooks.org/feeds/opds"),
    (
        "Feedbooks (Public Domain)",
        "https://catalog.feedbooks.com/catalog/public_domain.atom",
    ),
];

pub const PRELOADED_RSS_SOURCES: &[(&str, &str)] = &[
    ("Hacker News", "https://news.ycombinator.com/rss"),
    ("Hacker News (Front Page)", "https://hnrss.org/frontpage"),
    ("Longform", "https://longform.org/rss/"),
];

pub fn all_preloaded_sources() -> Vec<(&'static str, &'static str, FeedType)> {
    let mut sources = Vec::new();
    for (name, url) in PRELOADED_OPDS_SOURCES {
        sources.push((*name, *url, FeedType::Opds));
    }
    for (name, url) in PRELOADED_RSS_SOURCES {
        sources.push((*name, *url, FeedType::Rss));
    }
    sources
}

pub const JINA_READER_BASE: &str = "https://r.jina.ai/";

pub fn get_reader_url(article_url: &str) -> String {
    let mut result = String::with_capacity(JINA_READER_BASE.len() + article_url.len());
    result.push_str(JINA_READER_BASE);
    result.push_str(article_url);
    result
}
