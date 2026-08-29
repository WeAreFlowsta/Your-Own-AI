/**
 * Where a share got to. The app keeps the submission link the share
 * service returned (per character / skill, on this device) and reads the
 * submission's public state from GitHub the next time the maker looks -
 * an unauthenticated read of a public repository: no account, no email,
 * nothing collected. Worded for the maker, never "PR / merged / review".
 */
import type { ShareResult } from "./share";

const KEY = "yoai.shares";
export type ShareKind = "character" | "skill" | "mcp";

interface Remembered { pr_url: string; page: string; id: string; at: string }

function load(): Record<string, Remembered> {
  try { return JSON.parse(localStorage.getItem(KEY) || "{}"); } catch { return {}; }
}
function save(all: Record<string, Remembered>) {
  try { localStorage.setItem(KEY, JSON.stringify(all)); } catch { /* per-device convenience only */ }
}

export function rememberShare(kind: ShareKind, key: string, r: ShareResult) {
  const all = load();
  all[`${kind}:${key}`] = { pr_url: r.pr_url, page: r.page, id: r.id, at: new Date().toISOString() };
  save(all);
}
export function forgetShare(kind: ShareKind, key: string) {
  const all = load(); delete all[`${kind}:${key}`]; save(all);
}
export function rememberedShare(kind: ShareKind, key: string): Remembered | null {
  return load()[`${kind}:${key}`] ?? null;
}

export type ShareState = "checking" | "waiting" | "live" | "problem" | "closed";
export interface ShareStatus { state: ShareState; note?: string; page: string; pr_url: string }

/** One line for the maker. */
export function shareStatusText(s: ShareStatus, name: string): string {
  switch (s.state) {
    case "checking": return "Sent. We're checking it now.";
    case "waiting": return "Sent. Someone at Flowsta looks at every new share before it is listed - usually within a few days. You'll see it here.";
    case "live": return `${name} is listed. Anyone can find it at ${s.page.replace(/^https?:\/\//, "")}`;
    case "problem": return `Not added. Our check found a problem: ${s.note ?? "see the submission"}. Fix it and share again.`;
    case "closed": return s.note ? `Not added. The note from Flowsta: "${s.note}". Change it and share again.` : "Not added this time. Change it and share again.";
  }
}

const REVIEW_MARK = "**AI review**";

/** Read the submission's state. Null when GitHub can't be reached (offline). */
export async function fetchShareStatus(r: Remembered): Promise<ShareStatus | null> {
  const m = r.pr_url.match(/github\.com\/([^/]+)\/([^/]+)\/pull\/(\d+)/);
  if (!m) return null;
  const api = `https://api.github.com/repos/${m[1]}/${m[2]}`;
  const headers = { Accept: "application/vnd.github+json" };
  try {
    const pr = await (await fetch(`${api}/pulls/${m[3]}`, { headers, cache: "no-cache" })).json();
    if (typeof pr?.state !== "string") return null;
    const comments: { body?: string; user?: { login?: string; type?: string } }[] =
      await (await fetch(`${api}/issues/${m[3]}/comments?per_page=50`, { headers, cache: "no-cache" })).json().catch(() => []);
    const review = Array.isArray(comments) ? comments.find((c) => c.body?.startsWith(REVIEW_MARK)) : undefined;
    const human = Array.isArray(comments)
      ? [...comments].reverse().find((c) => c.body && !c.body.startsWith(REVIEW_MARK) && c.user?.type !== "Bot")
      : undefined;
    const base = { page: r.page, pr_url: r.pr_url };
    if (pr.merged || pr.merged_at) return { ...base, state: "live" };
    if (pr.state === "closed") return { ...base, state: "closed", note: human?.body?.trim().slice(0, 240) };
    const verdict = review?.body?.match(/\*\*(clear|look|block)\*\*/i)?.[1]?.toLowerCase();
    if (verdict === "block") {
      const reasons = review!.body!.split("\n").filter((l) => l.trim().startsWith("-")).map((l) => l.replace(/^\s*-\s*/, "").trim()).slice(0, 3).join("; ");
      return { ...base, state: "problem", note: reasons || undefined };
    }
    return { ...base, state: review ? "waiting" : "checking" };
  } catch {
    return null;
  }
}
