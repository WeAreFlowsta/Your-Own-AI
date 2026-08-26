// Daily watch on the engines we pin and the upstream work we are waiting for.
//
// Reads the pins from source (LLAMA_ENGINE_TAG, SWIFTLM_TAG) and the watch
// list from scripts/llama-watch.json, asks GitHub what changed, and prints a
// report. "News" - a new versioned llama.cpp release, a watched PR merged or
// closed, a new SwiftLM release past our pin - is decided against the `seen`
// JSON handed in via LLAMA_WATCH_SEEN (the workflow keeps it in the tracking
// issue's body, so nothing is committed by a bot).
//
// Outputs (files in the working directory):
//   watch-report.md   the full report, always
//   watch-news.md     only the lines that are new since `seen` (empty = no news)
//   watch-seen.json   the updated `seen` state to store back
//
// Run locally: GH_TOKEN=$(gh auth token) node scripts/watch-llama-cpp.mjs

import { readFileSync, writeFileSync } from "node:fs";

const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN || "";
const headers = { "User-Agent": "your-own-ai-llama-watch", Accept: "application/vnd.github+json" };
if (token) headers.Authorization = `Bearer ${token}`;

async function gh(path) {
  const r = await fetch(`https://api.github.com${path}`, { headers });
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return r.json();
}

const engineRs = readFileSync("src-tauri/src/engine.rs", "utf8");
const pinnedLlama = (engineRs.match(/LLAMA_ENGINE_TAG: &str = "llama-(b\d+)"/) || [])[1] || "?";
const mlxRs = readFileSync("src-tauri/src/mlx_engine.rs", "utf8");
const pinnedSwift = (mlxRs.match(/SWIFTLM_TAG: &str = "(b\d+)"/) || [])[1] || "?";
const watch = JSON.parse(readFileSync("scripts/llama-watch.json", "utf8"));

let seen = { releases: [], prs: {}, swiftlm: [] };
try {
  if (process.env.LLAMA_WATCH_SEEN) seen = { ...seen, ...JSON.parse(process.env.LLAMA_WATCH_SEEN) };
} catch {
  /* start fresh */
}

const report = [];
const news = [];

// --- llama.cpp versioned releases ------------------------------------------
const rels = await gh("/repos/ggml-org/llama.cpp/releases?per_page=40");
const versioned = rels.filter((r) => /^v\d+\.\d+\.\d+$/.test(r.tag_name));
report.push(`## llama.cpp`);
report.push(`Pinned engine: **${pinnedLlama}**. Versioned releases (newest first):`);
for (const r of versioned.slice(0, 5)) {
  const nightly = (r.body || "").match(/Nightly build:\*?\*?\s*\[?(b\d+)/)?.[1] || "?";
  const line = `- ${r.tag_name} (${r.published_at.slice(0, 10)}, nightly ${nightly}) ${r.html_url}`;
  report.push(line);
  if (!seen.releases.includes(r.tag_name)) {
    news.push(`New llama.cpp release **${r.tag_name}** (${r.published_at.slice(0, 10)}, nightly ${nightly}): ${r.html_url}`);
    seen.releases.push(r.tag_name);
  }
}

// --- watched PRs ------------------------------------------------------------
report.push(``, `## Watched pull requests`);
for (const w of watch.prs) {
  const pr = await gh(`/repos/ggml-org/llama.cpp/pulls/${w.number}`);
  const state = pr.merged_at ? `merged ${pr.merged_at.slice(0, 10)}` : pr.state === "closed" ? "closed" : pr.draft ? "open (draft)" : "open";
  report.push(`- #${w.number} ${pr.title} - **${state}** - ${w.why}`);
  const prev = seen.prs[String(w.number)];
  const key = pr.merged_at ? "merged" : pr.state;
  if (prev && prev !== key) {
    news.push(`PR #${w.number} (${w.why}) is now **${state}**: ${pr.html_url}`);
  }
  seen.prs[String(w.number)] = key;
}

// --- SwiftLM releases -------------------------------------------------------
const swift = await gh(`/repos/${watch.swiftlm.repo}/releases?per_page=10`);
report.push(``, `## SwiftLM (MLX preview)`);
report.push(`Pinned: **${pinnedSwift}**. Latest:`);
for (const r of swift.slice(0, 3)) {
  report.push(`- ${r.tag_name} (${r.published_at.slice(0, 10)}) ${r.html_url}`);
}
const swiftNum = (t) => Number((t.match(/b(\d+)/) || [])[1] || 0);
for (const r of swift) {
  if (swiftNum(r.tag_name) > swiftNum(pinnedSwift) && !seen.swiftlm.includes(r.tag_name)) {
    const first = (r.body || "").split("\n").find((l) => l.trim().startsWith("-")) || "";
    news.push(`New SwiftLM release **${r.tag_name}** past our pin ${pinnedSwift}: ${r.html_url} ${first.trim()}`);
    seen.swiftlm.push(r.tag_name);
  }
}

report.push(``, `_Checked ${new Date().toISOString().slice(0, 16)} UTC. Pins read from source; watch list in scripts/llama-watch.json._`);

writeFileSync("watch-report.md", report.join("\n") + "\n");
writeFileSync("watch-news.md", news.length ? news.map((n) => `- ${n}`).join("\n") + "\n" : "");
writeFileSync("watch-seen.json", JSON.stringify(seen));
console.log(report.join("\n"));
console.log(news.length ? `\nNEWS:\n${news.map((n) => "- " + n).join("\n")}` : "\n(no news)");
