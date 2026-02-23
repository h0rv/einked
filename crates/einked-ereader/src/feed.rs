extern crate alloc;

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
