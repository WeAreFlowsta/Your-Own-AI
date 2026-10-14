//! A project's identity (build-docs MEMORY_REDESIGN.md §13.1, decided
//! 2026-10-11): what makes "the same folder on two machines" one project,
//! so its memory rows meet. A git repo is its ROOT COMMIT hash - the same
//! on every clone, unmoved by renames, moves, remote changes or branches
//! (only a rewritten history changes it). A plain folder carries a small id
//! file, `.your-own-ai/project.json`, made on first use, which travels with
//! the folder. A folder that cannot be written falls back to a hash of its
//! path. Never the remote URL: a rename on the host changes it.

use std::path::Path;

const ID_FILE_DIR: &str = ".your-own-ai";
const ID_FILE: &str = "project.json";

/// The id, with its origin as a prefix: `git:<root commit>`, `id:<hex>`,
/// `path:<hash>`.
pub fn project_identity(folder: &Path) -> String {
    if let Some(root) = git_root_commit(folder) {
        return format!("git:{root}");
    }
    if let Some(id) = read_or_make_id_file(folder) {
        return format!("id:{id}");
    }
    format!("path:{}", path_hash(folder))
}

/// The earliest root commit of the repo the folder is in, if git says so.
fn git_root_commit(folder: &Path) -> Option<String> {
    let git = crate::mcp::which_sync("git")?;
    let out = std::process::Command::new(git)
        .args(["-C", &folder.to_string_lossy(), "rev-list", "--max-parents=0", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    pick_root(&String::from_utf8_lossy(&out.stdout))
}

/// Several root commits (an orphan branch merged in) - the smallest hash
/// is the stable choice on every clone.
pub(crate) fn pick_root(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| l.len() == 40 && l.bytes().all(|b| b.is_ascii_hexdigit()))
        .min()
        .map(String::from)
}

fn read_or_make_id_file(folder: &Path) -> Option<String> {
    let dir = folder.join(ID_FILE_DIR);
    let path = dir.join(ID_FILE);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(id) = v["id"].as_str().filter(|s| s.len() >= 16 && s.bytes().all(|b| b.is_ascii_hexdigit())) {
                return Some(id.to_string());
            }
        }
    }
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    let id = hex::encode(b);
    std::fs::create_dir_all(&dir).ok()?;
    let body = serde_json::json!({
        "_readme": "Your Own AI: this folder's id, so its project memory is the same project on every machine. Keep it with the folder; safe to commit.",
        "id": id,
        "made_at": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
    });
    std::fs::write(&path, serde_json::to_string_pretty(&body).ok()?).ok()?;
    Some(id)
}

fn path_hash(folder: &Path) -> String {
    use sha2::Digest;
    let canonical = folder.canonicalize().unwrap_or_else(|_| folder.to_path_buf());
    hex::encode(sha2::Sha256::digest(canonical.to_string_lossy().as_bytes()))[..32].to_string()
}

/// The folder's project id, for the front end (its memory scope is
/// `project:<id>`).
#[tauri::command]
pub fn project_identity_for(folder: String) -> String {
    project_identity(Path::new(&folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_smallest_root_commit_wins_and_noise_is_ignored() {
        let out = "ffffffffffffffffffffffffffffffffffffffff\n0123456789abcdef0123456789abcdef01234567\nnot a hash\n";
        assert_eq!(pick_root(out).as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
        assert!(pick_root("").is_none());
    }

    #[test]
    fn a_plain_folder_gets_an_id_file_once_and_reads_it_back_a_path_hash_never() {
        let dir = std::env::temp_dir().join(format!("yoai-pid-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = project_identity(&dir);
        let b = project_identity(&dir);
        assert!(a.starts_with("id:"), "{a}");
        assert_eq!(a, b, "the id file is read back");
        assert!(dir.join(".your-own-ai/project.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn this_repo_is_identified_by_its_root_commit() {
        // The app's own source tree is a git repo on the dev box; elsewhere
        // (a tarball) the id file path is taken instead - both are ids.
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let id = project_identity(here);
        assert!(id.starts_with("git:") || id.starts_with("id:"), "{id}");
        if id.starts_with("git:") {
            assert_eq!(id.len(), 4 + 40);
        }
    }
}

/// "Save as AGENTS.md" (MEMORY_REDESIGN.md §8): the project's pinned rules
/// appended to `<folder>/AGENTS.md` under "How to work here"; lines the file
/// already has are skipped, nothing is removed. Your Own AI never writes
/// this file unasked - this command IS the person's ask.
#[tauri::command]
pub fn save_rules_as_agents_md(folder: String, lines: Vec<String>) -> Result<serde_json::Value, String> {
    let path = Path::new(&folder).join("AGENTS.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let (next, added) = append_rules(&existing, &lines);
    if added > 0 {
        std::fs::write(&path, next).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    }
    log::info!("[memory] AGENTS.md in {folder}: {added} line(s) added");
    Ok(serde_json::json!({ "added": added, "path": path.to_string_lossy() }))
}

/// The file with `lines` added as bullets under "## How to work here" - a
/// line the file already carries (as a bullet or plain, case-insensitive)
/// is not added again. Returns the new text and how many lines were added.
pub(crate) fn append_rules(existing: &str, lines: &[String]) -> (String, usize) {
    let have: std::collections::HashSet<String> = existing
        .lines()
        .map(|l| l.trim().trim_start_matches("- ").trim().to_lowercase())
        .collect();
    let mut seen = std::collections::HashSet::new();
    let fresh: Vec<String> = lines
        .iter()
        .map(|l| l.trim().to_string())
        .filter(|t| !t.is_empty() && !have.contains(&t.to_lowercase()) && seen.insert(t.to_lowercase()))
        .collect();
    if fresh.is_empty() {
        return (existing.to_string(), 0);
    }
    let mut out = if existing.trim().is_empty() {
        "# AGENTS.md\n\nRules for working in this project, kept by the people who work here.\n".to_string()
    } else {
        existing.to_string()
    };
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.contains("## How to work here") {
        out.push_str("\n## How to work here\n\n");
    }
    for l in &fresh {
        out.push_str(&format!("- {l}\n"));
    }
    (out, fresh.len())
}

#[cfg(test)]
mod agents_md_tests {
    use super::append_rules;

    #[test]
    fn creates_appends_and_never_repeats() {
        let rules = vec!["Run the tests before every commit".to_string(), "Never force-push".to_string()];
        let (first, added) = append_rules("", &rules);
        assert_eq!(added, 2);
        assert!(first.starts_with("# AGENTS.md"));
        assert!(first.contains("## How to work here\n\n- Run the tests before every commit\n- Never force-push\n"));
        let (again, added) = append_rules(&first, &rules);
        assert_eq!(added, 0, "nothing new, nothing written");
        assert_eq!(again, first);
        let (third, added) = append_rules(&first, &vec!["never FORCE-push".to_string(), "Use US spelling".to_string()]);
        assert_eq!(added, 1, "a line already there in other case is not repeated");
        assert!(third.ends_with("- Use US spelling\n"));
        // A hand-written file keeps every line it had.
        let hand = "# My project\n\nBuild with make.\n";
        let (kept, added) = append_rules(hand, &vec!["Build with make.".to_string(), "Lint first".to_string()]);
        assert_eq!(added, 1);
        assert!(kept.starts_with(hand));
    }
}
