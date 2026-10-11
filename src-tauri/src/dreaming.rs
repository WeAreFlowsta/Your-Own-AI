//! Dreaming (build-docs MEMORY_REDESIGN.md §6): consolidation of the
//! person's memory rows when the app is idle - local, deterministic here
//! (the model-assisted parts, relative dates and splitting double
//! statements, join with the memory slot). Merge rows that say the same
//! thing (newest wins, history free), type notes that are plainly commands
//! or locations, retire the oldest layer-2 rows past the budget. Rules and
//! pinned rows are never touched. Every change is a revision with
//! `provenance: dream:<ids>`; running twice changes nothing. Never at
//! session end, never on the hot path, never online.

use crate::memory_rows::{reduce, says_the_same, MemoryRow};
use std::sync::atomic::{AtomicI64, Ordering};

/// Layer 2 (everything that is not a rule or pinned) keeps at most this
/// many live rows per scope; the oldest, least recent go to history.
pub const LAYER2_BUDGET_ROWS: usize = 500;
/// A dream needs this much quiet first.
const IDLE_SECS: i64 = 10 * 60;
/// And runs at most this often per scope.
const EVERY_SECS: i64 = 6 * 60 * 60;

static LAST_ACTIVITY_MS: AtomicI64 = AtomicI64::new(0);

/// Called on every chat turn (in-app and over :11435): the app is busy.
pub fn note_activity() {
    LAST_ACTIVITY_MS.store(now_ms(), Ordering::Relaxed);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn idle_for_secs() -> i64 {
    let last = LAST_ACTIVITY_MS.load(Ordering::Relaxed);
    if last == 0 {
        return i64::MAX; // never used since start: idle
    }
    (now_ms() - last) / 1000
}

/// What a note plainly is, by its shape alone - conservative, so a dream
/// never mis-types: a shell command, or a place in a tree.
pub fn plain_kind(text: &str) -> Option<&'static str> {
    let t = text.trim();
    let lower = t.to_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    // Unambiguous tools stand alone; words that are also English verbs
    // (go, make, node, python) need a subcommand or a flag after them.
    const CMDS: &[&str] = &["npm", "npx", "cargo", "git", "docker", "pytest", "yarn", "pnpm", "mvn", "gradle", "kubectl", "terraform", "firebase", "gcloud", "aws"];
    const AMBIGUOUS: &[&str] = &["go", "make", "node", "python", "python3"];
    const SUBCOMMANDS: &[&str] = &["build", "run", "test", "install", "mod", "get", "fmt", "vet", "deploy", "clean", "start", "dev", "lint", "-m", "-c"];
    let second = lower.split_whitespace().nth(1).unwrap_or("");
    let tool_at = |w: &str, next: &str| CMDS.contains(&w) || AMBIGUOUS.contains(&w) && (next.starts_with('-') || SUBCOMMANDS.contains(&next));
    let third = lower.split_whitespace().nth(2).unwrap_or("");
    if t.starts_with('$') || t.starts_with("`") && t.ends_with('`') || tool_at(first, second) || first == "run" && tool_at(second, third) {
        return Some("command");
    }
    let path_like = t.split_whitespace().any(|w| (w.contains('/') && !w.starts_with("http")) && w.len() > 3 || w.ends_with(".rs") || w.ends_with(".ts") || w.ends_with(".tsx") || w.ends_with(".md") || w.ends_with(".json"));
    if path_like && (lower.contains(" is in ") || lower.contains(" lives in ") || lower.contains(" is at ") || lower.contains(" are in ") || lower.starts_with("the ") && lower.contains(" in ")) {
        return Some("location");
    }
    None
}

/// `dream:<ids>;<source>|<source>` - the dream's own mark first, then every
/// source it rests on (deduped, an earlier dream's own mark dropped so the
/// chain reads as the original records), so a reader never has to walk
/// the history to find the transcript record a memory came from.
pub fn with_sources(mark: &str, sources: &[&str]) -> String {
    let mut seen: Vec<String> = Vec::new();
    for s in sources {
        // A prior provenance may itself be "dream:…;a|b": keep only a|b.
        let tail = match s.split_once(';') {
            Some((head, rest)) if head.starts_with("dream:") => rest,
            _ => s,
        };
        for part in tail.split('|').map(str::trim).filter(|p| !p.is_empty() && !p.starts_with("dream:")) {
            if !seen.iter().any(|x| x == part) {
                seen.push(part.to_string());
            }
        }
    }
    if seen.is_empty() { mark.to_string() } else { format!("{mark};{}", seen.join("|")) }
}

/// The revisions one dream of a scope writes. Pure: the same input gives
/// the same plan, and the plan applied gives an input that plans nothing.
pub fn plan_dream(live: &[MemoryRow], now: i64, budget: usize) -> Vec<MemoryRow> {
    let live = reduce(live.to_vec());
    let mut out: Vec<MemoryRow> = Vec::new();
    let mut consumed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let touchable = |r: &MemoryRow| r.kind != "rule" && !r.pinned;
    // Episodes are distinct events however alike their words: never merged.
    let mergeable = |r: &MemoryRow| touchable(r) && r.kind != "episode";

    // 1. Merge rows that say the same thing (same kind): the newest is the
    //    survivor and takes the others into `replaces`.
    let mut by_kind: std::collections::BTreeMap<&str, Vec<&MemoryRow>> = Default::default();
    for r in live.iter().filter(|r| mergeable(r)) {
        by_kind.entry(r.kind.as_str()).or_default().push(r);
    }
    for (_, rows) in by_kind {
        let mut rows = rows;
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        let mut i = 0;
        while i < rows.len() {
            let survivor = rows[i];
            if consumed.contains(&survivor.id) {
                i += 1;
                continue;
            }
            let dups: Vec<&MemoryRow> = rows[i + 1..]
                .iter()
                .copied()
                .filter(|o| !consumed.contains(&o.id) && says_the_same(&survivor.text, &o.text))
                .collect();
            if !dups.is_empty() {
                let mut rev = survivor.clone();
                rev.replaces = dups.iter().map(|d| d.id.clone()).collect();
                rev.replaces.sort();
                let ids: Vec<&str> = std::iter::once(survivor.id.as_str()).chain(dups.iter().map(|d| d.id.as_str())).collect();
                // The sources stay on the row: the survivor's provenance and
                // every merged row's (a transcript record, an author, an
                // earlier dream) - so the link to where a memory came from
                // never has to be dug out of the history.
                let sources: Vec<&str> = std::iter::once(survivor.provenance.as_str()).chain(dups.iter().map(|d| d.provenance.as_str())).collect();
                rev.provenance = with_sources(&format!("dream:{}", ids.join(",")), &sources);
                rev.updated_at = now;
                rev.action_hash = None;
                for d in &dups {
                    consumed.insert(d.id.clone());
                }
                consumed.insert(survivor.id.clone());
                out.push(rev);
            }
            i += 1;
        }
    }

    // 2. Type notes that are plainly something else.
    let notes: Vec<MemoryRow> = live.iter().filter(|r| touchable(r) && r.kind == "note" && !consumed.contains(&r.id)).cloned().collect();
    for r in notes.iter() {
        if let Some(kind) = plain_kind(&r.text) {
            let mut rev = r.clone();
            rev.kind = kind.into();
            rev.provenance = with_sources(&format!("dream:{}", r.id), &[r.provenance.as_str()]);
            rev.updated_at = now;
            rev.action_hash = None;
            consumed.insert(r.id.clone());
            out.push(rev);
        }
    }

    // 3. The budget: past it, the oldest layer-2 rows go to history
    //    (`valid_to`), never deleted - episodes and notes first.
    let live_after: Vec<&MemoryRow> = live.iter().filter(|r| touchable(r) && r.valid_to.is_none() && !consumed.contains(&r.id)).collect();
    let over = live_after.len().saturating_sub(budget);
    if over > 0 {
        let mut candidates: Vec<&MemoryRow> = live_after.into_iter().filter(|r| r.kind == "episode" || r.kind == "note").collect();
        candidates.sort_by(|a, b| a.updated_at.cmp(&b.updated_at).then(a.id.cmp(&b.id)));
        for r in candidates.into_iter().take(over) {
            let mut rev = r.clone();
            rev.valid_to = Some(now);
            rev.provenance = with_sources(&format!("dream:{}", r.id), &[r.provenance.as_str()]);
            rev.updated_at = now;
            rev.action_hash = None;
            out.push(rev);
        }
    }
    out
}

/// One scope, one dream: read, plan, write, through `agent`.
async fn dream_scope(app: &tauri::AppHandle, manager: &crate::holochain::HolochainManager, agent: &str, scope: &str) -> Result<usize, String> {
    let live = crate::memory_rows::rows_live(app, manager, agent, scope).await?;
    let plan = plan_dream(&live, now_ms(), LAYER2_BUDGET_ROWS);
    for rev in &plan {
        crate::memory_rows::write_row_pub(manager, agent, rev).await?;
    }
    if !plan.is_empty() {
        // Refresh the cache with the result.
        let _ = crate::memory_rows::rows_live(app, manager, agent, scope).await;
    }
    Ok(plan.len())
}

/// Every half hour: when the app has been quiet for ten minutes, dream the
/// person's scope and each AI's, at most once in six hours per scope.
pub fn start_dreaming(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let mut last_by_scope: std::collections::HashMap<String, i64> = Default::default();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
            if idle_for_secs() < IDLE_SECS {
                continue;
            }
            let Some(hc) = app.try_state::<std::sync::Arc<crate::commands_holochain::HolochainState>>() else { continue };
            let Ok(manager) = hc.get() else { continue };
            let agents: Vec<String> = manager.agents.lock().await.keys().cloned().collect();
            let Some(any) = agents.first().cloned() else { continue };
            let mut scopes: Vec<(String, String)> = vec![("person".to_string(), any)];
            for a in &agents {
                scopes.push((format!("ai:{}", crate::commands_holochain::founding_key_for(&app, a)), a.clone()));
            }
            let now = now_ms();
            for (scope, agent) in scopes {
                if now - last_by_scope.get(&scope).copied().unwrap_or(0) < EVERY_SECS * 1000 {
                    continue;
                }
                if idle_for_secs() < IDLE_SECS {
                    break; // the person came back
                }
                match dream_scope(&app, &manager, &agent, &scope).await {
                    Ok(0) => {}
                    Ok(n) => log::info!("[dream] {}: {} revision(s)", &scope[..scope.len().min(12)], n),
                    Err(e) => log::debug!("[dream] {} skipped: {}", &scope[..scope.len().min(12)], e),
                }
                last_by_scope.insert(scope, now);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, kind: &str, text: &str, at: i64) -> MemoryRow {
        MemoryRow {
            id: id.into(), scope: "person".into(), kind: kind.into(), text: text.into(), structured: None, as_of: at, valid_to: None,
            saved_by: "person".into(), provenance: "authored".into(), pinned: false, topics: vec![], replaces: vec![], deleted: false,
            updated_at: at, action_hash: Some(format!("h{at}")),
        }
    }

    fn apply(live: &[MemoryRow], plan: &[MemoryRow]) -> Vec<MemoryRow> {
        let mut all = live.to_vec();
        all.extend(plan.iter().cloned().map(|mut r| { r.action_hash = Some(format!("d{}", r.updated_at)); r }));
        reduce(all)
    }

    #[test]
    fn a_dream_merges_duplicates_types_plain_notes_and_never_touches_rules_or_pinned() {
        let mut pinned = row("p", "note", "npm run build", 1);
        pinned.pinned = true;
        let live = vec![
            row("r", "rule", "always ask first", 1),
            row("n1", "note", "The API lives in src/api/", 10),
            row("c1", "command", "Run npm test before every commit", 10),
            row("c2", "command", "Always run npm test before each commit", 20),
            row("n2", "note", "cargo build --release", 5),
            pinned,
        ];
        let plan = plan_dream(&live, 100, 500);
        let kinds: Vec<(String, String)> = plan.iter().map(|r| (r.id.clone(), r.kind.clone())).collect();
        assert!(kinds.contains(&("c2".into(), "command".into())), "the newest duplicate survives: {kinds:?}");
        assert!(plan.iter().find(|r| r.id == "c2").unwrap().replaces == vec!["c1".to_string()]);
        assert!(kinds.contains(&("n1".into(), "location".into())));
        assert!(kinds.contains(&("n2".into(), "command".into())));
        assert!(!plan.iter().any(|r| r.id == "r" || r.id == "p"), "rules and pinned rows are never touched");
        assert!(plan.iter().all(|r| r.provenance.starts_with("dream:")));
        // The merged command keeps both sources after its dream mark.
        let merged = plan.iter().find(|r| r.id == "c2").unwrap();
        assert_eq!(merged.provenance, "dream:c2,c1;authored");
        // Idempotent: the plan applied plans nothing.
        let after = apply(&live, &plan);
        assert!(plan_dream(&after, 200, 500).is_empty());
        assert_eq!(after.iter().filter(|r| r.kind == "command").count(), 2, "one merged command + the typed note (pinned stays a note)");
    }

    #[test]
    fn past_the_budget_the_oldest_episodes_go_to_history_not_the_bin() {
        let mut live: Vec<MemoryRow> = (0..12).map(|i| row(&format!("e{i}"), "episode", &format!("session {i} did thing {i}"), 1000 + i)).collect();
        live.push(row("f", "fact", "likes tea", 5));
        let plan = plan_dream(&live, 5000, 10);
        let retired: Vec<&str> = plan.iter().filter(|r| r.valid_to.is_some()).map(|r| r.id.as_str()).collect();
        assert_eq!(retired, vec!["e0", "e1", "e2"], "13 touchable rows, budget 10: the three oldest episodes; the fact stays");
        assert!(plan.iter().all(|r| !r.deleted));
        let after = apply(&live, &plan);
        assert!(plan_dream(&after, 6000, 10).is_empty());
    }

    #[test]
    fn a_dream_revision_keeps_the_transcript_record_its_memory_came_from() {
        let mut from_chat = row("f1", "note", "npm run lint", 10);
        from_chat.provenance = "extracted:uhCkk1234".into();
        let mut also = row("f2", "note", "run npm run lint", 20);
        also.provenance = "extracted:uhCkk5678".into();
        let plan = plan_dream(&[from_chat, also], 100, 500);
        // Both rows are typed as commands; f2 (newer) merges f1 by meaning.
        let f2 = plan.iter().find(|r| r.id == "f2").unwrap();
        assert!(f2.provenance.starts_with("dream:"));
        assert!(f2.provenance.contains("extracted:uhCkk5678"), "{}", f2.provenance);
        assert!(f2.provenance.contains("extracted:uhCkk1234"), "the merged row's record stays named: {}", f2.provenance);
        // A second dream over a dreamed row does not pile up marks.
        assert_eq!(with_sources("dream:x", &["dream:f2,f1;extracted:a|extracted:b", "authored"]), "dream:x;extracted:a|extracted:b|authored");
    }

    #[test]
    fn plain_kinds_are_conservative() {
        assert_eq!(plain_kind("npm run build"), Some("command"));
        assert_eq!(plain_kind("$ make deploy"), Some("command"));
        assert_eq!(plain_kind("The config is in src-tauri/tauri.conf.json"), Some("location"));
        assert_eq!(plain_kind("I prefer short answers"), None);
        assert_eq!(plain_kind("Go for a run every morning"), None, "'go' as a verb is not the Go tool");
    }
}
