//! Markdown from a notes app, made ready for the splitter.
//!
//! The splitter works on paragraphs (blank-line separated) and sentences. A
//! notes vault gives it neither: a Logseq page is one unbroken run of `- `
//! bullets with little sentence punctuation, so it was cut every 900
//! characters, mid-word; YAML front matter and `key:: value` properties were
//! embedded as prose; `[[links]]` and `((block refs))` rode along as noise in
//! every vector and every quoted passage.
//!
//! What this does, for any Markdown file (it is harmless on ordinary prose):
//!  - front matter and page properties: `title`, `tags`, `alias(es)` become
//!    one plain first line; the rest (ids, collapsed flags, dates) is dropped;
//!  - an outliner page: every TOP-LEVEL bullet, with its children, becomes a
//!    paragraph, so a thought and its sub-points stay together;
//!  - `[[Page]]` -> `Page`, `[[page|shown]]` -> `shown`, `#[[a tag]]` ->
//!    `#a tag`, `((block-uuid))` -> gone, `![[embed]]` -> `embed`.
//! Nothing is rewritten on disk: this shapes what is embedded and quoted.

/// Properties worth keeping as words; everything else is bookkeeping.
const KEEP: &[&str] = &["title", "tags", "tag", "alias", "aliases"];

fn keep_property(key: &str, value: &str, out: &mut Vec<String>) {
    let k = key.trim().to_ascii_lowercase();
    let v = value.trim().trim_matches(|c| c == '[' || c == ']' || c == '"' || c == '\'').trim();
    if KEEP.contains(&k.as_str()) && !v.is_empty() {
        let label = if k.starts_with("tag") { "Tags" } else if k.starts_with("alias") { "Also called" } else { "Title" };
        out.push(format!("{label}: {}.", unlink(v)));
    }
}

/// Wiki links, embeds and block references, as plain words.
pub(super) fn unlink(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        let link = rest.find("[[");
        let block = rest.find("((");
        let next = match (link, block) {
            (Some(l), Some(b)) => Some(l.min(b)),
            (l, b) => l.or(b),
        };
        let Some(at) = next else {
            out.push_str(rest);
            break;
        };
        let is_link = rest[at..].starts_with("[[");
        let close = if is_link { "]]" } else { "))" };
        let Some(end) = rest[at + 2..].find(close) else {
            out.push_str(rest);
            break;
        };
        let inner = &rest[at + 2..at + 2 + end];
        let before = &rest[..at];
        // `![[embed]]`: the bang belongs to the link.
        out.push_str(before.strip_suffix('!').unwrap_or(before));
        if is_link {
            // `[[page|shown]]` shows `shown`; `[[page#heading]]` reads as both.
            let shown = inner.rsplit('|').next().unwrap_or(inner);
            out.push_str(&shown.replace('#', " - "));
        } else if !looks_like_block_id(inner) {
            // Not a block reference after all: ordinary double parentheses.
            out.push_str("((");
            out.push_str(inner);
            out.push_str("))");
        }
        rest = &rest[at + 2 + end + 2..];
    }
    out
}

fn looks_like_block_id(s: &str) -> bool {
    s.len() >= 8 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn bullet_depth(line: &str) -> Option<usize> {
    let indent = line.len() - line.trim_start().len();
    let t = line.trim_start();
    (t.starts_with("- ") || t == "-" || t.starts_with("* ") || t.starts_with("+ ")).then_some(indent)
}

/// A `key:: value` line (Logseq), with or without its bullet.
fn page_property(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start().trim_start_matches("- ").trim_start();
    let (k, v) = t.split_once("::")?;
    (!k.is_empty() && !k.contains(' ') && !k.contains('/') && k.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'))
        .then_some((k, v))
}

pub(super) fn prepare_markdown(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let mut lines: Vec<&str> = text.lines().collect();
    let mut head: Vec<String> = Vec::new();

    // YAML front matter.
    if lines.first().map(|l| l.trim() == "---").unwrap_or(false) {
        if let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---" || l.trim() == "...") {
            let mut list_key: Option<String> = None;
            let mut list: Vec<String> = Vec::new();
            for l in &lines[1..=end] {
                let item = l.trim_start();
                if let (Some(_), Some(v)) = (&list_key, item.strip_prefix("- ")) {
                    list.push(v.trim().to_string());
                    continue;
                }
                if let Some(k) = list_key.take() {
                    keep_property(&k, &list.join(", "), &mut head);
                    list.clear();
                }
                if let Some((k, v)) = l.split_once(':') {
                    if v.trim().is_empty() {
                        list_key = Some(k.to_string());
                    } else {
                        keep_property(k, v, &mut head);
                    }
                }
            }
            if let Some(k) = list_key {
                keep_property(&k, &list.join(", "), &mut head);
            }
            lines.drain(..=end + 1);
        }
    }

    let bullets = lines.iter().filter(|l| bullet_depth(l).is_some()).count();
    let filled = lines.iter().filter(|l| !l.trim().is_empty()).count();
    let outliner = filled > 0 && bullets * 10 >= filled * 6;
    let top = lines.iter().filter_map(|l| bullet_depth(l)).min().unwrap_or(0);

    let mut body: Vec<String> = Vec::new();
    for line in lines {
        if let Some((k, v)) = page_property(line) {
            keep_property(k, v, &mut head);
            continue;
        }
        let depth = bullet_depth(line);
        if outliner && depth == Some(top) && body.last().map(|l| !l.is_empty()).unwrap_or(false) {
            body.push(String::new());
        }
        let shown = if outliner {
            match depth {
                // The bullet mark is layout, not words.
                Some(_) => line.trim_start()[1..].trim_start().to_string(),
                None => line.trim().to_string(),
            }
        } else {
            line.to_string()
        };
        body.push(unlink(&shown));
    }

    let mut out = String::new();
    if !head.is_empty() {
        out.push_str(&head.join(" "));
        out.push_str("\n\n");
    }
    out.push_str(&body.join("\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_embeds_and_block_refs_become_words() {
        assert_eq!(unlink("see [[Tide Tables]] and [[boats/survey|the survey]]"), "see Tide Tables and the survey");
        assert_eq!(unlink("as in ((64f1c2a0-1b2c-4d5e-8f90-abcdef012345)) said"), "as in  said");
        assert_eq!(unlink("![[diagram.png]] then #[[long tag]]"), "diagram.png then #long tag");
        assert_eq!(unlink("math ((a+b)) stays"), "math ((a+b)) stays");
        assert_eq!(unlink("an [[unclosed link"), "an [[unclosed link");
        assert_eq!(unlink("[[Page#Heading]]"), "Page - Heading");
    }

    #[test]
    fn an_outliner_page_is_split_by_top_level_bullet_with_its_children() {
        let page = "title:: Harbor notes\nid:: 64f1c2a0\n- The harbor\n  - tidal range 4.2 meters\n  - [[Port Quillon]]\n- The boat\n  - survey quoted 1,850\n";
        let out = prepare_markdown(page);
        let paras: Vec<&str> = out.split("\n\n").collect();
        assert_eq!(paras.len(), 3, "{out}");
        assert_eq!(paras[0], "Title: Harbor notes.");
        assert_eq!(paras[1], "The harbor\ntidal range 4.2 meters\nPort Quillon");
        assert_eq!(paras[2], "The boat\nsurvey quoted 1,850");
        assert!(!out.contains("64f1c2a0"), "bookkeeping properties are dropped");
    }

    #[test]
    fn front_matter_keeps_title_and_tags_only() {
        let note = "---\ntitle: \"Garden log\"\ntags:\n  - garden\n  - beans\ncreated: 2026-09-01\n---\n\nMarigolds keep aphids away.\n\nWater every second day.\n";
        let out = prepare_markdown(note);
        assert!(out.starts_with("Title: Garden log. Tags: garden, beans.\n\n"), "{out}");
        assert!(!out.contains("created"));
        assert!(out.contains("Marigolds keep aphids away.\n\nWater every second day."));
    }

    #[test]
    fn ordinary_prose_with_a_short_list_is_left_as_prose() {
        let doc = "# Report\n\nFirst paragraph here.\n\n- one point\n\nSecond paragraph.\nIt has two lines.\nAnd a third: not a property.\n";
        let out = prepare_markdown(doc);
        assert_eq!(out, doc.trim_end_matches('\n'));
    }
}

#[cfg(test)]
mod splitter_tests {
    use super::super::{chunk_text, PASSAGE_CHARS};
    use super::prepare_markdown;

    #[test]
    fn a_long_logseq_page_is_never_cut_inside_a_word() {
        // One top-level bullet with many unpunctuated children: the shape
        // that used to be cut every 900 characters, mid-word.
        let mut page = String::from("- Boat survey findings\n");
        for i in 0..120 {
            page.push_str(&format!("  - item {i} hull plank starboard fastening corroded replace bronze\n"));
        }
        let text = prepare_markdown(&page);
        let passages = chunk_text(&text, PASSAGE_CHARS);
        assert!(passages.len() > 3);
        let words: std::collections::HashSet<&str> =
            ["item", "hull", "plank", "starboard", "fastening", "corroded", "replace", "bronze", "Boat", "survey", "findings"].into_iter().collect();
        for p in &passages {
            assert!(p.chars().count() <= PASSAGE_CHARS + 200, "a passage stays near its size");
            for w in p.split_whitespace() {
                assert!(words.contains(w) || w.parse::<u32>().is_ok(), "a cut word: {w:?}");
            }
        }
    }
}
