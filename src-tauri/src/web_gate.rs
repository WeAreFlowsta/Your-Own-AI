//! The exfiltration gate for an agent's page reads (decided
//! 2026-10-15). An agent - or a page it read that carries instructions -
//! can compose an address of its own with the person's text in it; fetched
//! silently, the data leaves the machine in the address itself. So a read
//! is allowed without asking only for an address the person gave (typed in
//! the conversation) or on a site a read page linked to; any other address
//! is the person's call, through the permission card. One set per agent
//! session, keyed by a gate id the bridge mints and sends with the reader's
//! calls. Hosts match by site: `example.com` covers `www.example.com` and
//! `docs.example.com`.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

static SETS: Mutex<Option<HashMap<String, HashSet<String>>>> = Mutex::new(None);

/// The registrable host of an http(s) address, lower-cased, without a
/// leading `www.`; None for anything that is not an http(s) address.
pub fn site_of(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(url.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?.to_ascii_lowercase();
    Some(host.strip_prefix("www.").unwrap_or(&host).to_string())
}

fn with_sets<R>(f: impl FnOnce(&mut HashMap<String, HashSet<String>>) -> R) -> R {
    let mut g = SETS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

/// Add the sites of `urls` to the gate's set; returns how many sites it holds.
pub fn allow(gate: &str, urls: impl IntoIterator<Item = String>) -> usize {
    if gate.is_empty() {
        return 0;
    }
    with_sets(|m| {
        let set = m.entry(gate.to_string()).or_default();
        for u in urls {
            if let Some(site) = site_of(&u) {
                set.insert(site);
            }
        }
        set.len()
    })
}

/// True when `url` is on a site the gate holds (the site itself or any of
/// its subdomains). An empty gate id (no session) allows nothing silently.
pub fn allowed(gate: &str, url: &str) -> bool {
    if gate.is_empty() {
        return false;
    }
    let Some(host) = site_of(url) else { return false };
    with_sets(|m| {
        m.get(gate)
            .map(|set| set.iter().any(|s| host == *s || host.ends_with(&format!(".{s}"))))
            .unwrap_or(false)
    })
}

/// Drop a session's set.
pub fn forget(gate: &str) {
    if gate.is_empty() {
        return;
    }
    with_sets(|m| {
        m.remove(gate);
    });
}

const TLDS: &[&str] = &[
    "com", "net", "org", "io", "ai", "co", "dev", "app", "info", "edu", "gov", "biz", "me", "tv", "xyz", "site",
    "online", "tech", "shop", "store", "news", "blog", "page", "cloud", "wiki", "uk", "au", "nz", "de", "fr", "nl",
    "ca", "us", "eu", "ie", "es", "it", "se", "no", "dk", "fi", "ch", "at", "be", "pt", "pl", "cz", "jp", "kr", "in",
    "br", "mx", "za", "sg", "hk",
];

/// Web addresses in a person's words, the same rules as the chat's
/// `findWebAddresses`: full links, and bare domains the way people type
/// them ("what is on flowsta.com"); file names and e-mail addresses are not
/// addresses. Each result is a full address.
pub fn addresses_in_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '(' | ')' | '"' | '\'' | '[' | ']' | '`')) {
        let tok = raw.trim_end_matches(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?'));
        if tok.is_empty() || tok.contains('@') {
            continue;
        }
        let lower = tok.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            if site_of(tok).is_some() {
                out.push(tok.to_string());
            }
            continue;
        }
        let (host, rest) = match tok.find('/') {
            Some(i) => (&tok[..i], &tok[i..]),
            None => (tok, ""),
        };
        let labels: Vec<&str> = host.split('.').collect();
        if labels.len() < 2
            || labels.iter().any(|l| l.is_empty() || !l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        {
            continue;
        }
        let tld = labels[labels.len() - 1].to_ascii_lowercase();
        if !TLDS.contains(&tld.as_str()) {
            continue;
        }
        out.push(format!("https://{host}{rest}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_follow_the_chat_rules() {
        let found = addresses_in_text("what is on flowsta.com and https://example.org/a?b=1, not notes.md or e.g. or me@mail.com");
        assert_eq!(found, vec!["https://flowsta.com".to_string(), "https://example.org/a?b=1".to_string()]);
        assert!(addresses_in_text("open model.gguf and README.txt").is_empty());
        assert_eq!(addresses_in_text("see www.bbc.co.uk/news."), vec!["https://www.bbc.co.uk/news".to_string()]);
    }

    #[test]
    fn a_site_covers_its_subdomains_and_nothing_else() {
        let g = "gate-test-1";
        forget(g);
        assert!(!allowed(g, "https://example.com/"), "empty set allows nothing");
        assert_eq!(allow(g, vec!["https://www.example.com/about".to_string()]), 1);
        assert!(allowed(g, "https://example.com/"));
        assert!(allowed(g, "https://docs.example.com/x?q=1"));
        assert!(!allowed(g, "https://example.com.evil.net/"));
        assert!(!allowed(g, "https://notexample.com/"));
        assert!(!allowed(g, "ftp://example.com/"));
        assert!(!allowed("", "https://example.com/"), "no session, no silent read");
        forget(g);
        assert!(!allowed(g, "https://example.com/"));
    }
}
