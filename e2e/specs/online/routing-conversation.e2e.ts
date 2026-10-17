// Routing in real conversations: does the chat hand over at the right
// times, in both modes? Three conversations on one AI, every turn answered
// for real, each judged on the side that answered (the app's own decision
// record), the reason it gave, how soon the first words came and whether a
// model had to reload.
//
//   Offline only           - every turn on the device; the staging proxy
//                            sees no request at all, signed in or not.
//   Online and offline,    - everyday, news and code online; a follow-up
//   default dial             keeps its side; health stays on the device.
//   Online and offline,    - only what needs live web goes online, its
//   most private dial        follow-up stays there, the next ordinary
//                            question comes back to the device.
//
// Online turns run on GPT-6 Luna except the news turns (the search model).
// Timings go to e2e/shots/routing-conversation.json.
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { SHOTS } from "../../wdio.conf";
import { shot, invoke, waitForChat, ask, lastReply, resetAiModels } from "../models/_helpers";
import { cheapPicks, decisions, haveIdentity, logBetween, onlineRequests, setAiModel, setRoutingPrefs, signInAsTestIdentity } from "./_online";

type Want = "device" | "online" | "fresh" | "inherit";
type Turn = { q: string; want: Want };
type Row = { turn: number; q: string; want: Want; side: string; model: string; reason: string; ttftMs: number; totalMs: number; loads: number; webFetches: number };

const report: Record<string, Row[] | Record<string, unknown>> = {};
const save = () => writeFileSync(resolve(SHOTS, "routing-conversation.json"), JSON.stringify(report, null, 2));

const AI_TIMEOUT = 300_000;

async function newChat() {
  if (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length > 0)) {
    await (await $('[data-testid="new-chat"]')).click();
    await browser.waitUntil(() => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 15_000, timeoutMsg: "New chat did not clear the page" });
  }
  await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 30_000 });
}

async function converse(label: string, key: string, mode: "auto:offline" | "auto:online-offline", share: string, turns: Turn[]) {
  await setAiModel(key, mode);
  await setRoutingPrefs({ routingOnlineShare: share });
  await browser.url("tauri://localhost/chat/");
  await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
  await newChat();
  const rows: Row[] = [];
  for (const [i, t] of turns.entries()) {
    const before = (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length)) as number;
    const fromIso = new Date().toISOString();
    const t0 = Date.now();
    await ask(t.q);
    // First words: a new assistant message with text in it.
    let ttft = 0;
    await browser.waitUntil(async () => {
      const n = (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length)) as number;
      const r = await lastReply();
      if (n > before && r && r.text.trim().length > 0 && !ttft) ttft = Date.now() - t0;
      return n > before && r?.state === "done";
    }, { timeout: AI_TIMEOUT, interval: 250, timeoutMsg: `${label} turn ${i + 1} never finished` });
    const total = Date.now() - t0;
    const d = (await decisions()).filter((x) => x.at_ms >= t0 - 2_000).pop();
    const lines = logBetween(fromIso, new Date().toISOString());
    const row: Row = {
      turn: i + 1,
      q: t.q,
      want: t.want,
      side: d?.side ?? "?",
      model: d?.model ?? "?",
      reason: d?.reason ?? "?",
      ttftMs: ttft || total,
      totalMs: total,
      loads: lines.filter((l) => l.includes("[LLM] load_model '")).length,
      webFetches: lines.filter((l) => /\[web\] (fetch|read|search)/i.test(l)).length,
    };
    rows.push(row);
    console.log(`e2e: ${label} ${row.turn} [${row.side}] ${row.model} - ${row.reason} | first words ${row.ttftMs} ms, done ${row.totalMs} ms, loads ${row.loads}, web ${row.webFetches} | ${t.q}`);
  }
  report[label] = rows;
  save();
  await shot(`routing-${label}`);
  return rows;
}

/** Judge each turn against what it should have done. */
function judge(rows: Row[]) {
  const wrong: string[] = [];
  for (const [i, r] of rows.entries()) {
    const prev = rows[i - 1];
    const ok =
      r.want === "device" ? r.side === "device"
      : r.want === "online" ? r.side === "online"
      : r.want === "fresh" ? r.side === "online" && /search|sonar|web/i.test(r.model)
      : /* inherit */ !!prev && r.side === prev.side && /continuing the conversation/i.test(r.reason);
    if (!ok) wrong.push(`turn ${r.turn} wanted ${r.want}, got ${r.side} ${r.model} (${r.reason}): ${r.q}`);
  }
  return wrong;
}

/** Device turns that reloaded the same model the last device turn used. */
function needlessReloads(rows: Row[]) {
  const out: string[] = [];
  let last = "";
  for (const r of rows) {
    if (r.side !== "device") continue;
    if (r.loads > 0 && r.model === last) out.push(`turn ${r.turn} reloaded ${r.model}`);
    last = r.model;
  }
  return out;
}

describe("routing in real conversations", () => {
  let key = "";
  let ready = false;

  before(async function () {
    this.timeout(600_000);
    if (!haveIdentity()) return;
    await waitForChat();
    const session = await signInAsTestIdentity();
    expect(session.tier).toBe("pro");
    await setRoutingPrefs({ ...cheapPicks(), onlineEntitled: "yes" });
    key = (await browser.execute(async () => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      const ais = ((await store.get("custom-ais")) || []) as { agentPubKey?: string }[];
      return ais.find((a) => a.agentPubKey)?.agentPubKey ?? "";
    })) as string;
    // The helper model is not needed here; the run's 8 GB cap is.
    await invoke("stop_utility_server").catch(() => {});
    ready = !!key;
  });

  after(async () => {
    await setRoutingPrefs({ routingOnlineShare: null });
    await resetAiModels();
    save();
  });

  it("offline only: every turn on the device, and nothing reaches the proxy", async function () {
    if (!ready) this.skip();
    this.timeout(2_400_000);
    const requestsBefore = await onlineRequests();
    const rows = await converse("offline", key, "auto:offline", "frontier", [
      { q: "what's a good name for a cat", want: "device" },
      { q: "what are today's top news headlines", want: "device" },
      { q: "tell me more about the first one", want: "device" },
      { q: "can you explain my blood test results, my cholesterol is 240", want: "device" },
      { q: "write a Python function that reverses the words in a sentence", want: "device" },
    ]);
    await browser.pause(5_000);
    const requestsAfter = await onlineRequests();
    report["offline: proxy requests"] = { before: requestsBefore, after: requestsAfter };
    console.log(`e2e: proxy requests for the test plan ${requestsBefore} -> ${requestsAfter}`);
    expect(judge(rows)).toEqual([]);
    for (const r of rows) expect(r.reason.toLowerCase()).not.toContain("online:");
    expect(requestsAfter).toBe(requestsBefore);
    // A specialist (the medical model, a coder) may load by design; reloading
    // the model the previous device turn already had never should.
    expect(needlessReloads(rows)).toEqual([]);
  });

  it("online and offline, default dial: hands over and back at the right turns", async function () {
    if (!ready) this.skip();
    this.timeout(2_400_000);
    const rows = await converse("online-default", key, "auto:online-offline", "frontier", [
      { q: "what's a good name for a cat", want: "online" },
      { q: "what are today's top news headlines", want: "fresh" },
      { q: "tell me more about the first one", want: "inherit" },
      { q: "can you explain my blood test results, my cholesterol is 240", want: "device" },
      { q: "write a Python function that reverses the words in a sentence", want: "online" },
    ]);
    expect(judge(rows)).toEqual([]);
    // Handing back to the device never reloads the model it already holds.
    expect(needlessReloads(rows)).toEqual([]);
  });

  it("online and offline, most private dial: only live-web questions go online, then it comes back", async function () {
    if (!ready) this.skip();
    this.timeout(2_400_000);
    const rows = await converse("online-private", key, "auto:online-offline", "local", [
      { q: "what's a good name for a dog", want: "device" },
      { q: "what's the weather today in Chicago", want: "fresh" },
      { q: "and what about tomorrow?", want: "inherit" },
      { q: "what's a good name for a hamster", want: "device" },
    ]);
    expect(judge(rows)).toEqual([]);
  });
});
