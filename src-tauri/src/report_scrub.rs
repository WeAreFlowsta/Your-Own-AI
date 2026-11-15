//! What may leave the computer in a report a person sends us by hand.
//!
//! The truth matrix writes one text file. Every line of it passes through
//! `Scrub::line` before it reaches the file, the window or the log, whatever
//! the code that wrote the line put in it: a second net under the legs,
//! which are written to carry no private data in the first place. It
//! removes the home folder and the OS account name, any path, any address
//! that is not this computer's own, email addresses, Holochain keys and
//! hashes, and the names of model files that are not in our catalog (a
//! model the person made or named themselves).

/// What this computer would otherwise give away, gathered once per report.
pub struct Scrub {
    /// The home folder as the OS spells it, and with the other slash.
    homes: Vec<String>,
    /// The OS account name (and the home folder's own name), lower case.
    users: Vec<String>,
    /// A model file that is not in the catalog, and what the report calls it.
    own_models: Vec<(String, String)>,
}

fn catalog_holds(name: &str) -> bool {
    include_str!("../../src/data/recommended-models.ts").contains(&format!("'{name}'"))
}

impl Scrub {
    /// For this computer: the home folder and account from the environment,
    /// and `model_names` (files and folders in the models folder) checked
    /// against the catalog.
    pub fn for_this_computer(model_names: &[String]) -> Self {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let home = env("USERPROFILE").or_else(|| env("HOME")).unwrap_or_default();
        let mut users: Vec<String> = ["USER", "USERNAME", "LOGNAME"].iter().filter_map(|k| env(k)).collect();
        if let Some(last) = home.rsplit(['/', '\\']).next() {
            users.push(last.to_string());
        }
        Self::new(&home, &users, model_names)
    }

    pub fn new(home: &str, users: &[String], model_names: &[String]) -> Self {
        let mut homes = Vec::new();
        if home.len() > 3 {
            homes.push(home.to_string());
            let flipped = if home.contains('\\') { home.replace('\\', "/") } else { home.replace('/', "\\") };
            if flipped != home {
                homes.push(flipped);
            }
        }
        let mut users: Vec<String> = users.iter().map(|u| u.trim().to_lowercase()).filter(|u| u.chars().count() >= 3).collect();
        users.sort();
        users.dedup();
        let mut own: Vec<&String> = model_names.iter().filter(|n| !catalog_holds(n)).collect();
        own.sort();
        own.dedup();
        // The longest name first: one own name may contain another.
        let mut own_models: Vec<(String, String)> = own
            .iter()
            .enumerate()
            .map(|(i, n)| ((*n).clone(), format!("your-own-model-{}{}", i + 1, if n.to_lowercase().ends_with(".gguf") { ".gguf" } else { "" })))
            .collect();
        own_models.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Scrub { homes, users, own_models }
    }

    /// One report line, safe to send.
    pub fn line(&self, line: &str) -> String {
        let mut s = line.to_string();
        for h in &self.homes {
            s = replace_ignoring_case(&s, h, "~");
        }
        for (name, alias) in &self.own_models {
            s = s.replace(name.as_str(), alias);
        }
        // Word by word, the spacing kept (the report's columns are padded).
        s = s.split(' ').map(scrub_word).collect::<Vec<_>>().join(" ");
        for u in &self.users {
            s = replace_whole_word(&s, u, "<user>");
        }
        s
    }
}

fn replace_ignoring_case(s: &str, needle: &str, with: &str) -> String {
    let (ls, ln) = (s.to_lowercase(), needle.to_lowercase());
    // Lower-casing can change a string's byte length in rare alphabets:
    // then only the exact spelling is replaced.
    if ls.len() != s.len() || ln.is_empty() {
        return s.replace(needle, with);
    }
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(at) = ls[i..].find(&ln) {
        out.push_str(&s[i..i + at]);
        out.push_str(with);
        i += at + ln.len();
    }
    out.push_str(&s[i..]);
    out
}

fn replace_whole_word(s: &str, word_lower: &str, with: &str) -> String {
    let ls = s.to_lowercase();
    if ls.len() != s.len() {
        return s.to_string();
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(at) = ls[i..].find(word_lower) {
        let (start, end) = (i + at, i + at + word_lower.len());
        let before = s[..start].chars().next_back().map(is_word).unwrap_or(false);
        let after = s[end..].chars().next().map(is_word).unwrap_or(false);
        out.push_str(&s[i..start]);
        out.push_str(if before || after { &s[start..end] } else { with });
        i = end;
    }
    out.push_str(&s[i..]);
    out
}

/// A Holochain key or hash: `uhC` and at least thirty more base64url characters.
fn holds_holochain_id(w: &str) -> bool {
    w.match_indices("uhC").any(|(at, _)| w[at..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').count() >= 33)
}

fn is_ipv4(w: &str) -> bool {
    let host = w.split(':').next().unwrap_or("");
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
}

fn own_computer(host: &str) -> bool {
    let h = host.split(':').next().unwrap_or("");
    matches!(h, "127.0.0.1" | "localhost" | "0.0.0.0") || host.starts_with("[::1]")
}

/// A bare host name (`news.example.org`, with a port or not): dotted parts
/// of letters, digits and hyphens ending in letters, that is not a file
/// name the report prints and not one of our own public servers.
fn is_host_name(w: &str) -> bool {
    let host = w.split(':').next().unwrap_or("").to_lowercase();
    let parts: Vec<&str> = host.split('.').collect();
    let Some(last) = parts.last() else { return false };
    if parts.len() < 2 || !(2..=12).contains(&last.len()) || !last.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if !parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
        return false;
    }
    const FILE_ENDINGS: &[&str] = &["gguf", "js", "json", "md", "txt", "rs", "ts", "tsx", "py", "exe", "dll", "so", "dylib", "log", "yaml", "yml", "toml", "gitignore", "bin", "safetensors", "png", "pdf", "html", "css"];
    if FILE_ENDINGS.contains(last) {
        return false;
    }
    !(host.ends_with("flowsta.com") || host.ends_with("yourownai.net"))
}

fn scrub_word(word: &str) -> String {
    // Brackets, quotes and sentence marks around a word stay where they are.
    let trim: &[char] = &['(', ')', '[', ']', '{', '}', '"', '\'', '`', ',', ';', '<', '>'];
    let core = word.trim_matches(trim).trim_end_matches(['.', ':', '!', '?']);
    if core.is_empty() {
        return word.to_string();
    }
    let put = |with: &str| word.replacen(core, with, 1);
    if let Some(at) = core.find("://") {
        let rest = &core[at + 3..];
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        return if own_computer(host) { put(&format!("{}://{host}", &core[..at])) } else { put("<address>") };
    }
    if holds_holochain_id(core) {
        return put("<key>");
    }
    if let Some(at) = core.find('@') {
        if at > 0 && core[at + 1..].contains('.') {
            return put("<email>");
        }
    }
    if is_ipv4(core) && !own_computer(core) {
        return put("<address>");
    }
    if is_host_name(core) {
        return put("<address>");
    }
    let b = core.as_bytes();
    let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let slashes = core.matches('/').count();
    if core.contains('\\') || drive || core.starts_with("~/") || (core.starts_with('/') && core.len() > 1 && core[1..].chars().any(|c| c.is_alphabetic())) || slashes >= 2 {
        return put("<path>");
    }
    word.to_string()
}

#[cfg(test)]
mod tests {
    use super::Scrub;

    fn scrub() -> Scrub {
        Scrub::new(
            "/home/erin",
            &["erin".into()],
            &["gemma-4-E2B-it-Q4_K_M.gguf".into(), "erin-diary-tune-Q4.gguf".into()],
        )
    }

    /// Everything that names the person or where they are is gone; what the
    /// report is for (figures, catalog model names, this computer's own
    /// port) stays.
    #[test]
    fn a_report_line_carries_nothing_that_names_the_person() {
        let s = scrub();
        let gone = |line: &str, secret: &str| {
            let out = s.line(line);
            assert!(!out.to_lowercase().contains(&secret.to_lowercase()), "{secret:?} left in: {out}");
            out
        };
        gone("models: /home/erin/Models/big (3 GGUF)", "erin");
        gone("models: /HOME/Erin/Models", "erin");
        gone("models: /media/erin/SSD/models (3 GGUF)", "erin");
        gone("models: /run/media/erin/T7/models", "media");
        gone("could not open D:\\Erin\\Models\\x.gguf", "Erin");
        gone("could not open C:/Users/erin/models/x.gguf", "users");
        gone("could not open C:\\Users\\ERINDO~1\\AppData\\Local\\Temp\\x", "ERINDO");
        gone("Bundled engine missing at D:\\Apps\\Your Own AI\\bin", "Apps");
        let key = "uhCAk7x1Q2w3E4r5T6y7U8i9O0pAsDfGhJkLzXcVbNm1234567890abcd";
        let out = gone(&format!("other devices: not read (vault unreachable: error sending request for url (http://127.0.0.1:27777/backup/list?expected_identity={key}))"), key);
        assert!(out.contains("http://127.0.0.1:27777"), "this computer's own port stays: {out}");
        gone(&format!("Zome call failed: agent {key} not found"), key);
        gone("news.example.org answered 403 at https://news.example.org/a/b?token=abc", "example");
        gone("signed in as erin.d@example.com", "example.com");
        gone("peer 203.0.113.9:4433 did not answer", "203.0.113.9");
        gone("FAIL r1 | auto | open | online | external:/srv/erin/models/tune.gguf | no plan", "srv");
        gone("erin-diary-tune-Q4.gguf: 18/20 points", "diary");
        gone("the host ERIN-PC did not answer", "erin");
    }

    #[test]
    fn what_the_report_is_for_stays() {
        let s = scrub();
        for line in [
            "quality: gemma-4-E2B-it-Q4_K_M.gguf: 45/48 points (q1 2, q2 2) | registry overall 7 | 1075 tokens at 30.6 tok/s in 35 s",
            "RAM: 15.0 GB | models: in your home folder (3 GGUF, 0 MLX)",
            "gemma-4-E2B-it-Q4_K_M.gguf                 ok (2 scenarios)",
            "Your Own AI 0.9.1 | linux x86_64 | vulkan | llama-server-x86_64-unknown-linux-gnu",
            "Vault: on port 27777",
            "this device: 3 AI(s) | AIs backed up 4 min ago",
            "agent grade 6; project tasks 7/8 right tool (8/8 called one); required calls 3/3",
            "keeper: bootstrap.flowsta.com | reached 3 of 3",
            "figure: {\"first_reply_s\":2.2,\"kind\":\"first_reply\",\"model\":\"gemma-4-E2B-it-Q4_K_M.gguf\",\"place\":\"card\",\"read_tps\":349.0,\"write_tps\":47.8}",
            "figure: {\"app\":\"0.9.1\",\"cpu\":\"Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz\",\"gpus\":\"NVIDIA GeForce GTX 1050 Ti (4096 MiB free)\",\"kind\":\"machine\",\"os_version\":\"Linux (Ubuntu 26.04)\"}",
            "CPU: Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz | 6 cores, 12 threads | OS: Linux (Ubuntu 26.04)",
            "first reply: gemma-4-E2B-it-Q4_K_M.gguf | card | 2.7 s | writes 38.9 tok/s | reads 260 tok/s (498 tokens of instructions) | loaded in 2 s",
            "\"Show me what is in README.md.\" -> read_file",
            "2026-10-21 10:33:34  unhandled ReferenceError @ q-BC92XIMX.js:2:28375",
            "Hi Sam, I'm sorry you're feeling this way. e.g. try a walk at 7.30 a.m.",
        ] {
            assert_eq!(s.line(line), line);
        }
        assert_eq!(s.line("erin-diary-tune-Q4.gguf: ok"), "your-own-model-1.gguf: ok");
    }
}
