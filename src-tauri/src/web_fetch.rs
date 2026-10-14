//! Reading a web page for an AI, on this computer (NEXT.md "Web access per
//! AI - fetch for everyone, local, free").
//!
//! The page is downloaded here - nothing goes through Flowsta, no plan or
//! identity is needed - and handed back as text, or read by the local
//! helper model when it is long or a question was asked, so a big page never
//! floods a small model's window. Rules, in order: http is upgraded to
//! https; an address with a user name, no host, a dotless host (localhost,
//! .local, a bare word) or a local / private / link-local address is REFUSED
//! - every address the name resolves to is checked, and the connection is
//! pinned to those addresses so a second lookup cannot land elsewhere; a
//! redirect to another host is reported, not followed; the body is capped.
//! A fetched page or a model must never reach this computer's own servers,
//! the Vault's loopback included.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION};
use reqwest::Url;
use tauri::AppHandle;

/// The most of a page that is read.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_HOPS: usize = 3;
/// A page up to this long is handed over as text when no question was asked.
const DIRECT_CHARS: usize = 12_000;
/// What the helper reads of a long page.
const HELPER_PAGE_CHARS: usize = 24_000;
const USER_AGENT: &str = "Your Own AI (reads pages for one person; https://yourownai.net)";

#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    NotAnAddress,
    Scheme(String),
    Userinfo,
    NoHost,
    Dotless(String),
    LocalAddress { host: String, ip: String },
}

impl Refusal {
    pub fn message(&self) -> String {
        match self {
            Refusal::NotAnAddress => "That is not a web address.".into(),
            Refusal::Scheme(s) => format!("Only web pages (https) can be read, not {s} addresses."),
            Refusal::Userinfo => "Addresses with a user name in them are not read.".into(),
            Refusal::NoHost => "That address has no site in it, or the site could not be found.".into(),
            Refusal::Dotless(h) => format!("{h} is a local name, not a public site - pages on this computer or its network are never read."),
            Refusal::LocalAddress { host, ip } => format!("{host} points at a local address ({ip}) - pages on this computer or its network are never read."),
        }
    }
}

/// Is this an address on this computer or its own network? Loopback,
/// private, link-local, carrier-grade NAT, unspecified, broadcast, multicast
/// and documentation ranges - IPv4-mapped IPv6 included.
pub fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (64..=127).contains(&o[1]))
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_local_ip(IpAddr::V4(mapped));
            }
            let s = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        }
    }
}

/// Parse and vet an address before any network I/O: https only (http is
/// upgraded), no user name, a host with a dot that is not a local name; an
/// IP literal is checked as an address.
pub fn check_url(raw: &str) -> Result<Url, Refusal> {
    let raw = raw.trim().trim_matches(|c| c == '<' || c == '>' || c == '"' || c == '\'');
    if raw.is_empty() {
        return Err(Refusal::NotAnAddress);
    }
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    let mut url = Url::parse(&with_scheme).map_err(|_| Refusal::NotAnAddress)?;
    match url.scheme() {
        "https" => {}
        "http" => {
            url.set_scheme("https").map_err(|_| Refusal::NotAnAddress)?;
            if url.port() == Some(80) {
                let _ = url.set_port(None);
            }
        }
        s => return Err(Refusal::Scheme(s.to_string())),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Refusal::Userinfo);
    }
    let host = url.host_str().ok_or(Refusal::NoHost)?.to_string();
    let bare = host.trim_matches(|c| c == '[' || c == ']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        if is_local_ip(ip) {
            return Err(Refusal::LocalAddress { host: host.clone(), ip: ip.to_string() });
        }
        return Ok(url);
    }
    let lower = host.to_ascii_lowercase();
    if !lower.contains('.')
        || lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
        || lower.ends_with(".home.arpa")
    {
        return Err(Refusal::Dotless(host));
    }
    Ok(url)
}

/// Every address the host resolves to must be public; the list comes back
/// so the connection can be pinned to it.
pub async fn resolve_checked(url: &Url) -> Result<Vec<IpAddr>, Refusal> {
    let host = url.host_str().ok_or(Refusal::NoHost)?.to_string();
    let bare = host.trim_matches(|c| c == '[' || c == ']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return if is_local_ip(ip) {
            Err(Refusal::LocalAddress { host, ip: ip.to_string() })
        } else {
            Ok(vec![ip])
        };
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| Refusal::NoHost)?
        .map(|a| a.ip())
        .collect();
    if addrs.is_empty() {
        return Err(Refusal::NoHost);
    }
    if let Some(bad) = addrs.iter().find(|ip| is_local_ip(**ip)) {
        return Err(Refusal::LocalAddress { host, ip: bad.to_string() });
    }
    Ok(addrs)
}

/// The text of a page: scripts, styles and the head dropped, the main
/// article preferred when the page marks one, block tags as line breaks,
/// entities decoded, whitespace collapsed.
pub fn html_to_text(html: &str) -> String {
    let mut s = strip_blocks(html, &["script", "style", "noscript", "svg", "head", "template"]);
    s = strip_comments(&s);
    let lower = s.to_ascii_lowercase();
    for tag in ["<main", "<article"] {
        if let Some(start) = lower.find(tag) {
            let close = format!("</{}", &tag[1..]);
            if let Some(end) = lower[start..].find(&close) {
                s = s[start..start + end].to_string();
                break;
            }
        }
    }
    const BLOCK: &[&str] = &[
        "p", "div", "br", "li", "ul", "ol", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "td", "th", "table",
        "section", "article", "header", "footer", "nav", "aside", "blockquote", "pre", "hr", "dt", "dd", "figcaption",
    ];
    let mut out = String::with_capacity(s.len() / 2);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            for t in chars.by_ref() {
                if t == '>' {
                    break;
                }
                tag.push(t);
            }
            let closing = tag.starts_with('/');
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            // A block OPENS a line; its close is just a gap (both would double every break).
            out.push(if !closing && BLOCK.contains(&name.as_str()) { '\n' } else { ' ' });
        } else {
            out.push(c);
        }
    }
    let decoded = decode_entities(&out);
    let mut lines: Vec<String> = Vec::new();
    let mut blank = false;
    for line in decoded.lines() {
        let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            if !blank && !lines.is_empty() {
                lines.push(String::new());
            }
            blank = true;
        } else {
            lines.push(collapsed);
            blank = false;
        }
    }
    lines.join("\n").trim().to_string()
}

/// The page's title, if it has one.
pub fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = lower[start..].find('>')? + start + 1;
    let end = lower[open_end..].find("</title")? + open_end;
    let t = decode_entities(&html[open_end..end]).split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() { None } else { Some(t) }
}

fn strip_blocks(html: &str, tags: &[&str]) -> String {
    let mut s = html.to_string();
    for tag in tags {
        loop {
            let lower = s.to_ascii_lowercase();
            let open = format!("<{tag}");
            let Some(start) = lower.find(&open) else { break };
            // A real tag, not a prefix of another (<header is not <head).
            let after = lower[start + open.len()..].chars().next();
            if !matches!(after, Some('>') | Some(' ') | Some('\n') | Some('\t') | Some('/')) {
                // Skip this occurrence by cutting just the '<'.
                let mut next = s[..start].to_string();
                next.push('\u{1}');
                next.push_str(&s[start + 1..]);
                s = next;
                continue;
            }
            let close = format!("</{tag}");
            let end = match lower[start..].find(&close) {
                Some(i) => {
                    let e = start + i;
                    lower[e..].find('>').map(|j| e + j + 1).unwrap_or(s.len())
                }
                None => s.len(),
            };
            s = format!("{}{}", &s[..start], &s[end..]);
        }
    }
    s.replace('\u{1}', "<")
}

fn strip_comments(html: &str) -> String {
    let mut s = html.to_string();
    while let Some(start) = s.find("<!--") {
        let end = s[start..].find("-->").map(|i| start + i + 3).unwrap_or(s.len());
        s = format!("{}{}", &s[..start], &s[end..]);
    }
    s
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(semi) = tail.find(';').filter(|j| *j <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..semi];
        let decoded = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('-'),
            "ndash" => Some('-'),
            "hellip" => Some('…'),
            "rsquo" | "lsquo" => Some('\''),
            "ldquo" | "rdquo" => Some('"'),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A link on a page: its text and absolute address.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Link {
    pub text: String,
    pub url: String,
    /// On the same site as the page it was found on.
    pub same_site: bool,
}

/// A fetched page, as text.
#[derive(Debug, Clone)]
pub struct Page {
    pub url: String,
    pub title: Option<String>,
    pub text: String,
    pub truncated: bool,
    /// Outbound links, same-site first, de-duplicated, capped.
    pub links: Vec<Link>,
}

/// Under this much readable text a page is "thin" - a sign-in wall, a
/// script-drawn page, a stub - and one read was not enough.
pub const THIN_CHARS: usize = 600;

impl Page {
    pub fn is_thin(&self) -> bool {
        self.text.chars().count() < THIN_CHARS
    }
}

const MAX_LINKS: usize = 20;

/// The page's links as absolute addresses that pass the address rules,
/// same-site ones first, duplicates and fragments dropped.
pub fn extract_links(html: &str, base: &Url) -> Vec<Link> {
    let lower = html.to_ascii_lowercase();
    let host = base.host_str().unwrap_or("").to_ascii_lowercase();
    let mut seen = std::collections::HashSet::new();
    let mut same: Vec<Link> = Vec::new();
    let mut other: Vec<Link> = Vec::new();
    let mut at = 0;
    while let Some(i) = lower[at..].find("<a ") {
        let start = at + i;
        let Some(close) = lower[start..].find('>') else { break };
        let tag_end = start + close + 1;
        let tag = &html[start..tag_end];
        let text_end = lower[tag_end..].find("</a").map(|j| tag_end + j).unwrap_or(tag_end);
        at = text_end.max(tag_end);
        let Some(h) = lower[start..tag_end].find("href=") else { continue };
        let rest = &tag[h + 5..];
        let href: String = match rest.chars().next() {
            Some(q) if q == '"' || q == '\'' => rest[1..].chars().take_while(|c| *c != q).collect(),
            _ => rest.chars().take_while(|c| !c.is_whitespace() && *c != '>').collect(),
        };
        let href = decode_entities(href.trim());
        if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") || href.starts_with("mailto:") || href.starts_with("tel:") {
            continue;
        }
        let Ok(mut abs) = base.join(&href) else { continue };
        abs.set_fragment(None);
        let Ok(checked) = check_url(abs.as_str()) else { continue };
        let key = checked.as_str().trim_end_matches('/').to_string();
        if key == base.as_str().trim_end_matches('/') || !seen.insert(key) {
            continue;
        }
        let text = html_to_text(&html[tag_end..text_end]).split_whitespace().collect::<Vec<_>>().join(" ");
        let text = if text.is_empty() { checked.path().trim_matches('/').replace(['-', '_'], " ") } else { text };
        let same_site = checked.host_str().map(|h| h.eq_ignore_ascii_case(&host)).unwrap_or(false);
        let link = Link { text: text.chars().take(80).collect(), url: checked.to_string(), same_site };
        if same_site { same.push(link) } else { other.push(link) }
        if same.len() + other.len() >= MAX_LINKS * 3 {
            break;
        }
    }
    same.into_iter().chain(other).take(MAX_LINKS).collect()
}

static CACHE: Mutex<Option<HashMap<String, (Instant, Page)>>> = Mutex::new(None);

fn cache_get(url: &str) -> Option<Page> {
    let guard = CACHE.lock().ok()?;
    let map = guard.as_ref()?;
    let (at, page) = map.get(url)?;
    if at.elapsed() > CACHE_TTL {
        return None;
    }
    Some(page.clone())
}

fn cache_put(url: &str, page: &Page) {
    if let Ok(mut guard) = CACHE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.retain(|_, (at, _)| at.elapsed() <= CACHE_TTL);
        map.insert(url.to_string(), (Instant::now(), page.clone()));
    }
}

/// Download one page under the rules above and return its text.
pub async fn fetch_page(raw: &str) -> Result<Page, String> {
    let mut url = check_url(raw).map_err(|r| r.message())?;
    if let Some(p) = cache_get(url.as_str()) {
        return Ok(p);
    }
    let asked = url.to_string();
    for _hop in 0..MAX_HOPS {
        let addrs = resolve_checked(&url).await.map_err(|r| r.message())?;
        let host = url.host_str().unwrap_or("").to_string();
        let port = url.port_or_known_default().unwrap_or(443);
        let pinned: Vec<SocketAddr> = addrs.iter().map(|ip| SocketAddr::new(*ip, port)).collect();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TIMEOUT)
            .user_agent(USER_AGENT)
            .resolve_to_addrs(&host, &pinned)
            .build()
            .map_err(|e| format!("The reader could not start: {e}"))?;
        let mut resp = client
            .get(url.clone())
            .header(ACCEPT, "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.5")
            .send()
            .await
            .map_err(|e| format!("{host} could not be reached: {e}"))?;
        let status = resp.status();
        if status.is_redirection() {
            let loc = resp
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| format!("{host} redirected without saying where."))?;
            let next = url.join(loc).map_err(|_| format!("{host} redirected to an address that could not be read."))?;
            let next = check_url(next.as_str()).map_err(|r| r.message())?;
            let same_host = match (next.host_str(), url.host_str()) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                _ => false,
            };
            if !same_host {
                return Err(format!(
                    "{host} sends readers on to {} - ask for that address directly if you want it read.",
                    next.host_str().unwrap_or("another site")
                ));
            }
            url = next;
            continue;
        }
        if !status.is_success() {
            return Err(format!("{host} answered {status}."));
        }
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_text = ct.is_empty() || ct.contains("text/") || ct.contains("json") || ct.contains("xml");
        if !is_text {
            return Err(format!(
                "That address is a file ({}), not a page to read.",
                ct.split(';').next().unwrap_or(&ct).trim()
            ));
        }
        let mut body: Vec<u8> = Vec::new();
        let mut truncated = false;
        while let Some(chunk) = resp.chunk().await.map_err(|e| format!("{host} stopped sending: {e}"))? {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_BYTES {
                body.truncate(MAX_BYTES);
                truncated = true;
                break;
            }
        }
        let raw_text = String::from_utf8_lossy(&body).to_string();
        let looks_html = ct.contains("html") || (ct.is_empty() && raw_text.contains('<'));
        let (title, text, links) = if looks_html {
            (extract_title(&raw_text), html_to_text(&raw_text), extract_links(&raw_text, &url))
        } else {
            (None, raw_text.trim().to_string(), Vec::new())
        };
        let page = Page { url: url.to_string(), title, text, truncated, links };
        cache_put(&asked, &page);
        return Ok(page);
    }
    Err("Too many redirects.".into())
}

/// Read a page for an AI: the text itself when it is short and no question
/// was asked; otherwise the local helper model reads it and answers.
pub async fn read_web_page(app: &AppHandle, raw: &str, question: Option<&str>, gate: Option<&str>) -> Result<String, String> {
    let page = fetch_page(raw).await?;
    // The pages this page links to may be read next without asking (web_gate).
    if let Some(g) = gate {
        crate::web_gate::allow(g, std::iter::once(page.url.clone()).chain(page.links.iter().map(|l| l.url.clone())));
    }
    let head = format!(
        "{}{}\n\n",
        page.title.as_deref().map(|t| format!("{t}\n")).unwrap_or_default(),
        page.url
    );
    let q = question.map(str::trim).filter(|q| !q.is_empty());
    let len = page.text.chars().count();
    // The page's links, so a reader can choose where to go next.
    let links_block = if page.links.is_empty() {
        String::new()
    } else {
        let mut b = String::from("\n\nLinks on this page:\n");
        for l in page.links.iter().take(12) {
            b.push_str(&format!("- {} - {}\n", l.text, l.url));
        }
        b
    };
    if page.text.trim().is_empty() {
        return Ok(format!("{head}(The page has no readable text - it may draw its content with scripts.){links_block}"));
    }
    // A page the asking model can read itself goes over as text, question or
    // not: the helper only pays for itself on a long page (and loading it
    // beside an agent's model can take minutes on a small card).
    if len <= DIRECT_CHARS {
        return Ok(format!("{head}{}{}{links_block}", page.text, if page.truncated { "\n\n(The page was longer than the 2 MB cap; this is the start of it.)" } else { "" }));
    }
    let excerpt: String = page.text.chars().take(HELPER_PAGE_CHARS).collect();
    let system = "You read a web page for someone and answer from it. Use only what the page says; if the page does not answer the question, say so plainly. Keep names, numbers, dates and quotations exact. Plain prose, under 400 words.";
    let user = match q {
        Some(q) => format!("Question: {q}\n\nPage ({}):\n{excerpt}", page.url),
        None => format!("Summarize this page faithfully: what it is, what it says, the key facts.\n\nPage ({}):\n{excerpt}", page.url),
    };
    // Bounded: a tool call must never hold an agent's turn open for good.
    let read = tokio::time::timeout(
        Duration::from_secs(120),
        crate::llm::memory_complete(app, crate::llm::UTILITY_MODEL_FILE, system, &user, None, 700, 0.2),
    )
    .await
    .unwrap_or_else(|_| Err("the helper model took too long".into()));
    match read {
        Ok(answer) if !answer.trim().is_empty() => Ok(format!(
            "{head}{}\n\n(Read by the helper model from the page's first {} characters{}.){links_block}",
            answer.trim(),
            excerpt.chars().count().min(len),
            if len > HELPER_PAGE_CHARS { " of a longer page" } else { "" }
        )),
        other => {
            if let Err(e) = other {
                log::warn!("[web] helper could not read the page: {e}");
            }
            let cut: String = page.text.chars().take(DIRECT_CHARS).collect();
            Ok(format!(
                "{head}{cut}{}",
                if len > DIRECT_CHARS { "\n\n(The page text, cut at 12,000 characters.)" } else { "" }
            ))
        }
    }
}

/// The chat's own page reading (no agent): the page a person points at,
/// read on this computer under the rules above.
#[tauri::command]
pub async fn web_read_page(app: AppHandle, url: String, question: Option<String>) -> Result<String, String> {
    read_web_page(&app, &url, question.as_deref(), None).await
}

/// What the chat's research step needs besides the text: was the page thin,
/// and where could a reader go next.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PageRead {
    pub url: String,
    pub title: Option<String>,
    pub text: String,
    pub thin: bool,
    pub links: Vec<Link>,
}

#[tauri::command]
pub async fn web_read_page_full(app: AppHandle, url: String, question: Option<String>) -> Result<PageRead, String> {
    let page = fetch_page(&url).await?;
    let thin = page.is_thin();
    let text = read_web_page(&app, &url, question.as_deref(), None).await?;
    Ok(PageRead { url: page.url.clone(), title: page.title.clone(), text, thin, links: page.links.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_addresses_are_refused() {
        for raw in [
            "http://localhost/",
            "https://localhost:27777/",
            "http://127.0.0.1:11435/v1/models",
            "https://[::1]/",
            "http://10.0.0.5/",
            "http://192.168.1.1/admin",
            "http://172.16.0.9/",
            "http://169.254.169.254/latest/meta-data",
            "http://0.0.0.0/",
            "http://100.64.0.1/",
            "https://[::ffff:127.0.0.1]/",
            "https://[fe80::1]/",
            "https://[fd00::1]/",
            "http://vault/",
            "http://printer.local/",
            "http://box.internal/",
            "ftp://example.com/file",
            "https://user:pw@example.com/",
            "",
            "not an address at all",
        ] {
            assert!(check_url(raw).is_err(), "{raw} should be refused");
        }
    }

    #[test]
    fn public_addresses_pass_and_http_is_upgraded() {
        let u = check_url("http://example.com/path?q=1").unwrap();
        assert_eq!(u.as_str(), "https://example.com/path?q=1");
        let u = check_url("example.org").unwrap();
        assert_eq!(u.as_str(), "https://example.org/");
        assert!(check_url("https://8.8.8.8/").is_ok());
        assert!(check_url("https://docs.rs/reqwest").is_ok());
    }

    #[test]
    fn local_ip_ranges() {
        assert!(is_local_ip("127.0.0.1".parse().unwrap()));
        assert!(is_local_ip("10.1.2.3".parse().unwrap()));
        assert!(is_local_ip("::1".parse().unwrap()));
        assert!(is_local_ip("::ffff:192.168.0.1".parse().unwrap()));
        assert!(!is_local_ip("1.1.1.1".parse().unwrap()));
        assert!(!is_local_ip("2606:4700::1111".parse().unwrap()));
    }

    #[test]
    fn links_are_absolute_checked_and_same_site_first() {
        let base = Url::parse("https://shop.example.com/products/kettle").unwrap();
        let html = r##"<a href="/about">About us</a> <a href='specs#top'>Specs</a> <a href="https://other.example.org/x">Review</a>
            <a href="http://localhost:8080/admin">admin</a> <a href="#top">top</a> <a href="/about">About again</a> <a href="mailto:a@b.c">mail</a>"##;
        let links = extract_links(html, &base);
        let urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, vec!["https://shop.example.com/about", "https://shop.example.com/products/specs", "https://other.example.org/x"], "{urls:?}");
        assert!(links[0].same_site && !links[2].same_site);
        assert_eq!(links[0].text, "About us");
    }

    #[test]
    fn html_becomes_readable_text() {
        let html = r#"<html><head><title>Tea &amp; Time</title><style>p{}</style></head>
            <body><header>Menu</header><main><h1>Tea</h1><p>First&nbsp;paragraph.</p>
            <script>alert(1)</script><p>Second &lt;b&gt; paragraph &#8212; done.</p><ul><li>one</li><li>two</li></ul></main>
            <footer>foot</footer></body></html>"#;
        assert_eq!(extract_title(html).as_deref(), Some("Tea & Time"));
        let text = html_to_text(html);
        assert!(text.starts_with("Tea\nFirst paragraph."), "{text}");
        assert!(text.contains("Second <b> paragraph — done."), "{text}");
        assert!(text.contains("one\ntwo"), "{text}");
        assert!(!text.contains("alert"), "{text}");
        assert!(!text.contains("Menu"), "main is preferred: {text}");
        assert!(!text.contains("foot"), "{text}");
    }
}
