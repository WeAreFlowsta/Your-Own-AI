// "May already be done", measured on real cases, on the built app. A
// measurement, not a gate: it runs only when YOAI_E2E_DONE_CASES names a
// JSON file of cases, kept outside the repository. Each case is something
// left open and a later item on the same subject; in a "closed" case the
// later item reports it done in other words, in a "still-open" case it
// does not. Every open thing is kept first, so each later item is judged
// with all of them present, then the tidying pass is asked about each later
// item. Printed: how many closed cases were proposed as done, and every
// proposal that is wrong (a still-open case, or two items of different cases).
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { waitForChat, ask, waitForReplyDone, invoke } from "./_helpers";

type Side = { date: string };
type Case = { n: number; kind: "closed" | "still-open"; open: Side; later: Side; open_item: string; later_item: string };
type Item = { id: string };
const FILE = process.env.YOAI_E2E_DONE_CASES ?? "";
const OUT = process.env.YOAI_MEMORY_OUT ?? "";
const BASE = "http://127.0.0.1:11435/internal";

async function tidy(queue: string): Promise<{ ran: boolean; pairs: { earlier: string; later: string; verdict: string }[] }> {
  const floor = process.env.YOAI_E2E_DONE_FLOOR ? `&open_floor=${process.env.YOAI_E2E_DONE_FLOOR}` : "";
  const res = await fetch(`${BASE}/memory-tidy?force=1&queue=${encodeURIComponent(queue)}${floor}`, { method: "POST" });
  const answer = await res.json();
  if (!res.ok || answer?.error) throw new Error(`memory-tidy: ${JSON.stringify(answer?.error ?? res.status)}`);
  return answer.report;
}

async function keep(text: string, itemType: "open" | "decision", day: string, n: number): Promise<string> {
  // Each item in a conversation of its own, as of its day.
  const at = Date.parse(`${day.slice(0, 10)}T12:00:00Z`) + n * 1000;
  const kept = await invoke<{ item: Item }>("memory_keep", {
    input: { text, itemType, ptr: { c: at * 1000, s: 0, t: at * 1000, a: 0, b: 0 }, asOf: at, kind: "exchange", why: "pressed" },
  });
  return kept.item.id;
}

describe("may already be done, on real cases", () => {
  it("proposes what was done and leaves what is still open", async function () {
    if (!FILE) this.skip();
    this.timeout(4 * 3_600_000);
    const cases = JSON.parse(readFileSync(FILE, "utf8")) as Case[];
    await waitForChat();
    await ask("Name one primary colour and nothing else.");
    await waitForReplyDone();
    // How alike the two sides of each case are, as the search measures it
    // (search only): which pairs the pass will put together at all.
    if (process.env.YOAI_E2E_DONE_LIKENESS) {
      const texts = cases.flatMap((c) => [c.open_item, c.later_item]).map((t) => `title: none | text: ${t}`);
      const vectors = await invoke<number[][]>("embed_texts", { texts, model: "embeddinggemma-300M-Q8_0.gguf" });
      const cos = (a: number[], b: number[]) => {
        let d = 0, x = 0, y = 0;
        for (let i = 0; i < a.length; i++) { d += a[i] * b[i]; x += a[i] * a[i]; y += b[i] * b[i]; }
        return d / Math.sqrt(x * y);
      };
      const likeness = cases.map((c, i) => {
        const later = vectors[2 * i + 1];
        const others = cases.map((o, j) => ({ n: o.n, s: cos(vectors[2 * j], later) })).filter((o) => o.n !== c.n).sort((a, b) => b.s - a.s);
        return { n: c.n, kind: c.kind, own: Number(cos(vectors[2 * i], later).toFixed(3)), nextOther: Number(others[0].s.toFixed(3)), nextOtherCase: others[0].n };
      });
      console.log(`e2e: DONE likeness: ${JSON.stringify(likeness)}`);
      if (OUT) writeFileSync(resolve(OUT, "done-cases-likeness.json"), JSON.stringify(likeness, null, 1));
      if (process.env.YOAI_E2E_DONE_LIKENESS === "only") return;
    }
    const open: Record<number, string> = {};
    for (const c of cases) open[c.n] = await keep(c.open_item, "open", c.open.date, c.n);
    const caseOf = (id: string) => Number(Object.entries(open).find(([, v]) => v === id)?.[0] ?? 0);
    const rows: Record<string, unknown>[] = [];
    let proposed = 0;
    const wrong: string[] = [];
    for (const c of cases) {
      const later = await keep(c.later_item, "decision", c.later.date, 100 + c.n);
      const report = await tidy(later);
      const about = report.pairs.filter((p) => p.later === later && open[caseOf(p.earlier)] === p.earlier);
      const own = about.find((p) => caseOf(p.earlier) === c.n);
      const others = about.filter((p) => caseOf(p.earlier) !== c.n && p.verdict === "DONE").map((p) => caseOf(p.earlier));
      if (c.kind === "closed" && own?.verdict === "DONE") proposed++;
      if (c.kind === "still-open" && own?.verdict === "DONE") wrong.push(`case ${c.n} (still open) proposed as done`);
      for (const o of others) wrong.push(`case ${c.n}'s later item proposed case ${o} as done`);
      rows.push({ n: c.n, kind: c.kind, compared: !!own, verdict: own?.verdict ?? "not compared", others });
      console.log(`e2e: DONE case ${c.n} ${c.kind}: ${own ? own.verdict : "not compared"}${others.length ? `; also proposed ${others.join(",")}` : ""}`);
    }
    const closed = cases.filter((c) => c.kind === "closed").length;
    console.log(`e2e: MAY ALREADY BE DONE: ${proposed} of ${closed} closed cases proposed; ${wrong.length} wrong proposals${wrong.length ? `: ${wrong.join("; ")}` : ""}; not compared: ${rows.filter((r) => !r.compared).map((r) => r.n).join(",") || "none"}`);
    if (OUT) writeFileSync(resolve(OUT, "done-cases-results.json"), JSON.stringify(rows, null, 1));
  });
});
