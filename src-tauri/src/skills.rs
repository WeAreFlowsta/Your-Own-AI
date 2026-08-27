//! Skills - folders of instructions an AI reads when the work calls for it
//! (the Agent Skills open standard: `<skill>/SKILL.md` plus supporting
//! files). Installed skills live in `~/.your-own-ai-build/skills/<name>/`,
//! the Build agent's own user-level skills directory, so a project session
//! discovers them with no extra configuration; the chat path reads the
//! same folders and hands the SKILL.md text to the model directly (no tool
//! use there, so the whole file goes in).
//!
//! A skill is text; installing one never runs anything. Skills that ship
//! scripts, hooks or MCP servers are flagged `runs_programs` so the page
//! can say so - the agent's trust store keeps those blocked until trusted.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Provenance sidecar written next to SKILL.md on install (dotfile: the
/// loaders ignore it).
const SIDECAR: &str = ".your-own-ai-skill.json";
/// Refuse archives beyond this (a skill is text; 50 MB is already generous).
const MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;
const USER_AGENT: &str = "your-own-ai-skills";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SkillSource {
    /// "folder" | "zip" | "link"
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub installed_at: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub dir: String,
    pub files: usize,
    pub skill_md_chars: usize,
    /// SKILL.md size in tokens (the running model's tokenizer when one is
    /// up, a dense-text ratio otherwise) - what a chat turn pays for it.
    pub tokens: u64,
    pub runs_programs: bool,
    pub source: Option<SkillSource>,
}

/// `~/.your-own-ai-build/skills` - created on first use.
pub(crate) fn skills_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let home = app
        .path()
        .home_dir()
        .map_err(|e| format!("cannot resolve home dir: {e}"))?;
    let dir = home.join(".your-own-ai-build").join("skills");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create skills folder: {e}"))?;
    Ok(dir)
}

/// Skill names as the loaders key them: lowercase, `a-z0-9-`, no edge dashes.
pub(crate) fn normalize_name(raw: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in raw.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// `name` and `description` from SKILL.md front matter (`---` block), if any.
pub(crate) fn parse_front_matter(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut description = None;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches('"').trim_matches('\'').trim();
            match k.trim() {
                "name" if !v.is_empty() => name = Some(v.to_string()),
                "description" if !v.is_empty() && v != ">" && v != "|" => description = Some(v.to_string()),
                _ => {}
            }
        }
    }
    (name, description)
}

/// SKILL.md without its front matter - what the chat path hands the model.
pub(crate) fn body_without_front_matter(text: &str) -> &str {
    let trimmed = text.trim_start();
    if !trimmed.starts_with("---") {
        return text;
    }
    let after = &trimmed[3..];
    match after.find("\n---") {
        Some(i) => {
            let rest = &after[i + 4..];
            rest.trim_start_matches(|c| c == '\r' || c == '\n')
        }
        None => text,
    }
}

fn is_program_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == ".mcp.json" || name == "hooks.json" || name == "plugin.json" {
        return true;
    }
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("sh" | "py" | "js" | "ts" | "mjs" | "ps1" | "bat" | "cmd" | "exe" | "rb")
    )
}

fn skip_dir(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "target" | "__pycache__" | ".DS_Store")
}

/// (file count, runs_programs) over a skill folder.
fn scan_folder(root: &Path) -> (usize, bool) {
    let mut files = 0;
    let mut runs = false;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if skip_dir(&name) {
                    continue;
                }
                if name == "scripts" || name == "hooks" || name == "bin" {
                    runs = true;
                }
                stack.push(p);
            } else if name != SIDECAR {
                files += 1;
                if is_program_file(&p) {
                    runs = true;
                }
            }
        }
    }
    (files, runs)
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        let dest = to.join(&name);
        if p.is_dir() {
            if skip_dir(&name) {
                continue;
            }
            copy_dir(&p, &dest)?;
        } else {
            std::fs::copy(&p, &dest).map_err(|e| format!("copy {}: {e}", p.display()))?;
        }
    }
    Ok(())
}

fn read_sidecar(dir: &Path) -> Option<SkillSource> {
    let text = std::fs::read_to_string(dir.join(SIDECAR)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_sidecar(dir: &Path, source: &SkillSource) -> Result<(), String> {
    let text = serde_json::to_string_pretty(source).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(SIDECAR), text).map_err(|e| e.to_string())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn info_for(dir: &Path) -> Option<SkillInfo> {
    let skill_md = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let dir_name = dir.file_name()?.to_str()?.to_string();
    let (fm_name, fm_desc) = parse_front_matter(&skill_md);
    let name = fm_name.map(|n| normalize_name(&n)).filter(|n| !n.is_empty()).unwrap_or(dir_name);
    let description = fm_desc.unwrap_or_else(|| {
        // First non-empty, non-heading line of the body, capped.
        body_without_front_matter(&skill_md)
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .unwrap_or("")
            .chars()
            .take(200)
            .collect()
    });
    let (files, runs_programs) = scan_folder(dir);
    let body = body_without_front_matter(&skill_md);
    let tokens = crate::llm::count_tokens_text(body).await;
    Some(SkillInfo {
        name,
        description,
        dir: dir.to_string_lossy().to_string(),
        files,
        skill_md_chars: skill_md.chars().count(),
        tokens,
        runs_programs,
        source: read_sidecar(dir),
    })
}

/// Every installed skill (a folder with a SKILL.md), by name.
#[tauri::command]
pub async fn skills_list(app: AppHandle) -> Result<Vec<SkillInfo>, String> {
    let root = skills_dir(&app)?;
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&root) else { return Ok(out) };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for d in dirs {
        if d.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.')).unwrap_or(true) {
            continue;
        }
        if let Some(info) = info_for(&d).await {
            out.push(info);
        }
    }
    Ok(out)
}

/// Locate the skill root inside an extracted or picked folder: the folder
/// itself, else `sub` under a single top-level dir (GitHub archives), else
/// the shallowest SKILL.md anywhere below.
fn find_skill_root(base: &Path, sub: Option<&str>) -> Option<PathBuf> {
    if base.join("SKILL.md").is_file() {
        return Some(base.to_path_buf());
    }
    let tops: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    if let Some(sub) = sub.filter(|s| !s.is_empty()) {
        for t in &tops {
            let cand = t.join(sub);
            if cand.join("SKILL.md").is_file() {
                return Some(cand);
            }
        }
        if base.join(sub).join("SKILL.md").is_file() {
            return Some(base.join(sub));
        }
    }
    if tops.len() == 1 && tops[0].join("SKILL.md").is_file() {
        return Some(tops[0].clone());
    }
    // Shallowest SKILL.md below.
    let mut best: Option<(usize, PathBuf)> = None;
    let mut stack: Vec<(usize, PathBuf)> = vec![(0, base.to_path_buf())];
    while let Some((depth, dir)) = stack.pop() {
        if depth > 4 {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() && !skip_dir(&name) {
                stack.push((depth + 1, p));
            } else if name == "SKILL.md" && best.as_ref().map(|(d, _)| depth < *d).unwrap_or(true) {
                best = Some((depth, dir.clone()));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Move a validated skill folder into place under its final name.
fn install_from(app: &AppHandle, root: &Path, source: SkillSource) -> Result<String, String> {
    let skill_md = std::fs::read_to_string(root.join("SKILL.md"))
        .map_err(|_| "That folder has no SKILL.md - a skill starts with one.".to_string())?;
    let (fm_name, _) = parse_front_matter(&skill_md);
    let fallback = root.file_name().and_then(|n| n.to_str()).unwrap_or("skill");
    let name = fm_name
        .map(|n| normalize_name(&n))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| normalize_name(fallback));
    if name.is_empty() {
        return Err("Couldn't work out a name for this skill.".into());
    }
    let dest = skills_dir(app)?.join(&name);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| format!("replace {}: {e}", dest.display()))?;
    }
    copy_dir(root, &dest)?;
    write_sidecar(&dest, &source)?;
    log::info!("[skills] installed '{name}' from {} ({})", source.kind, source.url.clone().or(source.path.clone()).unwrap_or_default());
    Ok(name)
}

/// Add a skill from a folder on this computer (the folder holding SKILL.md,
/// or one whose single subfolder does).
#[tauri::command]
pub async fn skills_add_folder(app: AppHandle, path: String) -> Result<String, String> {
    let base = PathBuf::from(&path);
    if !base.is_dir() {
        return Err("That path is not a folder.".into());
    }
    let root = find_skill_root(&base, None).ok_or("No SKILL.md found in that folder.")?;
    let source = SkillSource { kind: "folder".into(), path: Some(path), installed_at: now_secs(), ..Default::default() };
    install_from(&app, &root, source)
}

fn extract_zip(bytes: Vec<u8>, into: &Path) -> Result<(), String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| format!("not a zip file: {e}"))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name refuses `..` and absolute paths (zip-slip).
        let Some(rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else { continue };
        let out = into.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn temp_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let t = skills_dir(app)?.join(format!(".tmp-{}", now_secs()));
    std::fs::create_dir_all(&t).map_err(|e| e.to_string())?;
    Ok(t)
}

async fn install_zip_bytes(app: &AppHandle, bytes: Vec<u8>, sub: Option<&str>, source: SkillSource) -> Result<String, String> {
    let tmp = temp_dir(app)?;
    let result = (|| {
        let t = tmp.clone();
        let b = bytes;
        extract_zip(b, &t)?;
        let root = find_skill_root(&t, sub).ok_or("No SKILL.md found in that archive.")?;
        install_from(app, &root, source)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// Add a skill from a zip file on this computer.
#[tauri::command]
pub async fn skills_add_zip(app: AppHandle, path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read that file: {e}"))?;
    let source = SkillSource { kind: "zip".into(), path: Some(path), installed_at: now_secs(), ..Default::default() };
    install_zip_bytes(&app, bytes, None, source).await
}

/// A GitHub URL taken apart: owner, repo, optional ref and sub-path
/// (`https://github.com/o/r`, `.../o/r/tree/<ref>/<path>`).
#[derive(Debug, PartialEq)]
pub(crate) struct GithubRef {
    pub owner: String,
    pub repo: String,
    pub r#ref: Option<String>,
    pub sub: Option<String>,
}

pub(crate) fn parse_github_url(url: &str) -> Option<GithubRef> {
    let u = url.trim().trim_end_matches('/');
    let rest = u
        .strip_prefix("https://github.com/")
        .or_else(|| u.strip_prefix("http://github.com/"))
        .or_else(|| u.strip_prefix("github.com/"))?;
    let mut parts = rest.split('/');
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.trim_end_matches(".git").to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    let mut r#ref = None;
    let mut sub = None;
    if matches!(parts.next(), Some("tree") | Some("blob")) {
        r#ref = parts.next().map(str::to_string);
        let s: Vec<&str> = parts.collect();
        if !s.is_empty() {
            sub = Some(s.join("/"));
        }
    }
    Some(GithubRef { owner, repo, r#ref, sub })
}

async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = client.get(url).send().await.map_err(|e| format!("download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download failed: HTTP {} for {url}", resp.status().as_u16()));
    }
    if resp.content_length().unwrap_or(0) > MAX_ARCHIVE_BYTES {
        return Err("That archive is larger than a skill should be (50 MB limit).".into());
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err("That archive is larger than a skill should be (50 MB limit).".into());
    }
    Ok(bytes.to_vec())
}

/// Add a skill from a link: a GitHub repository (optionally `/tree/<ref>/<path>`),
/// pinned to the commit it resolves to today, or a direct `.zip` URL.
#[tauri::command]
pub async fn skills_add_link(app: AppHandle, url: String) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    if let Some(gh) = parse_github_url(&url) {
        let api = format!("https://api.github.com/repos/{}/{}", gh.owner, gh.repo);
        let r#ref = match gh.r#ref.clone() {
            Some(r) => r,
            None => {
                let meta: serde_json::Value = client
                    .get(&api)
                    .send()
                    .await
                    .map_err(|e| format!("GitHub lookup failed: {e}"))?
                    .json()
                    .await
                    .map_err(|e| format!("GitHub lookup failed: {e}"))?;
                meta.get("default_branch")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .ok_or("Couldn't find that repository on GitHub.")?
            }
        };
        let commit: serde_json::Value = client
            .get(format!("{api}/commits/{}", r#ref))
            .send()
            .await
            .map_err(|e| format!("GitHub lookup failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("GitHub lookup failed: {e}"))?;
        let sha = commit
            .get("sha")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or("Couldn't resolve that branch or tag on GitHub.")?;
        let bytes = fetch_bytes(&client, &format!("https://codeload.github.com/{}/{}/zip/{sha}", gh.owner, gh.repo)).await?;
        let source = SkillSource {
            kind: "link".into(),
            url: Some(format!("https://github.com/{}/{}", gh.owner, gh.repo)),
            r#ref: Some(r#ref),
            sha: Some(sha),
            path: gh.sub.clone(),
            installed_at: now_secs(),
        };
        return install_zip_bytes(&app, bytes, gh.sub.as_deref(), source).await;
    }
    if url.trim().to_ascii_lowercase().ends_with(".zip") {
        let bytes = fetch_bytes(&client, url.trim()).await?;
        let source = SkillSource { kind: "link".into(), url: Some(url.trim().to_string()), installed_at: now_secs(), ..Default::default() };
        return install_zip_bytes(&app, bytes, None, source).await;
    }
    Err("Use a GitHub repository link or a direct link to a .zip file.".into())
}

/// Remove an installed skill (its folder).
#[tauri::command]
pub async fn skills_remove(app: AppHandle, name: String) -> Result<(), String> {
    let name = normalize_name(&name);
    if name.is_empty() {
        return Err("no skill name".into());
    }
    let dir = skills_dir(&app)?.join(&name);
    if dir.join("SKILL.md").is_file() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        log::info!("[skills] removed '{name}'");
    }
    Ok(())
}

/// SKILL.md of one installed skill (for a preview).
#[tauri::command]
pub async fn skills_skill_md(app: AppHandle, name: String) -> Result<String, String> {
    let dir = skills_dir(&app)?.join(normalize_name(&name));
    std::fs::read_to_string(dir.join("SKILL.md")).map_err(|e| e.to_string())
}

/// The chat path's skills block: every installed skill (or only `names`),
/// SKILL.md bodies under one heading. Empty string when there is nothing.
#[tauri::command]
pub async fn skills_prompt_block(app: AppHandle, names: Option<Vec<String>>) -> Result<String, String> {
    let root = skills_dir(&app)?;
    let wanted: Option<Vec<String>> = names.map(|v| v.iter().map(|n| normalize_name(n)).collect());
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.join("SKILL.md").is_file()).collect())
        .unwrap_or_default();
    dirs.sort();
    let mut block = String::new();
    for d in dirs {
        let dir_name = d.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if let Some(w) = &wanted {
            if !w.iter().any(|n| n == &dir_name) {
                continue;
            }
        }
        let Ok(text) = std::fs::read_to_string(d.join("SKILL.md")) else { continue };
        let (fm_name, _) = parse_front_matter(&text);
        let name = fm_name.unwrap_or(dir_name);
        let body = body_without_front_matter(&text).trim();
        if body.is_empty() {
            continue;
        }
        block.push_str(&format!("\n\n### Skill: {name}\n{body}"));
    }
    if block.is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "You have the following skills - instructions you follow when the task calls for them. Apply the relevant one; ignore the others.{block}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_and_body() {
        let t = "---\nname: Holochain\ndescription: \"hApp development\"\n---\n\n# Body\nhello";
        let (n, d) = parse_front_matter(t);
        assert_eq!(n.as_deref(), Some("Holochain"));
        assert_eq!(d.as_deref(), Some("hApp development"));
        assert_eq!(body_without_front_matter(t), "# Body\nhello");
        assert_eq!(parse_front_matter("# no front matter"), (None, None));
        assert_eq!(body_without_front_matter("# no front matter"), "# no front matter");
    }

    #[test]
    fn names_normalize_like_the_loader() {
        assert_eq!(normalize_name("Holochain Agent Skill"), "holochain-agent-skill");
        assert_eq!(normalize_name("  --Weird__Name!! "), "weird-name");
        assert_eq!(normalize_name(""), "");
    }

    #[test]
    fn github_urls() {
        assert_eq!(
            parse_github_url("https://github.com/Soushi888/holochain-agent-skills"),
            Some(GithubRef { owner: "Soushi888".into(), repo: "holochain-agent-skills".into(), r#ref: None, sub: None })
        );
        assert_eq!(
            parse_github_url("https://github.com/anthropics/skills/tree/main/skills/doc-coauthoring/"),
            Some(GithubRef { owner: "anthropics".into(), repo: "skills".into(), r#ref: Some("main".into()), sub: Some("skills/doc-coauthoring".into()) })
        );
        assert_eq!(parse_github_url("https://github.com/o/r.git").map(|g| g.repo), Some("r".into()));
        assert!(parse_github_url("https://example.com/x.zip").is_none());
    }

    #[test]
    fn finds_skill_root_in_archive_layouts() {
        let tmp = std::env::temp_dir().join(format!("yoai-skills-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let top = tmp.join("repo-abc123");
        std::fs::create_dir_all(top.join("skills").join("one")).unwrap();
        std::fs::write(top.join("skills").join("one").join("SKILL.md"), "---\nname: one\n---\nx").unwrap();
        assert_eq!(find_skill_root(&tmp, Some("skills/one")), Some(top.join("skills").join("one")));
        // No sub given: the shallowest SKILL.md wins.
        assert_eq!(find_skill_root(&tmp, None), Some(top.join("skills").join("one")));
        std::fs::write(top.join("SKILL.md"), "root").unwrap();
        assert_eq!(find_skill_root(&tmp, None), Some(top.clone()));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
