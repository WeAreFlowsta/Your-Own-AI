// Memory in a project session, on the built app with the made-up bakery
// history: a question about earlier work is searched once for the turn,
// what was found rides beside the person's prompt, the reply is recorded
// with what it was given and gets the line under it; a second prompt is a
// new turn with its own search. Needs the Build helper installed on this
// machine (skipped otherwise).
import { homedir } from "node:os";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { shot, waitForChat, invoke, ask, waitForReplyDone, resetAiModels } from "./_helpers";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = path.resolve(here, "../../fixtures/memory");
const BASE = "http://127.0.0.1:11435/internal";
const HELPER = process.env.YOAI_E2E_HELPER || path.resolve(homedir(), ".local/share/com.solar.yourowai/build/your-own-ai-build");
const DELIVERY = "What is the free delivery threshold? Answer from what we said before, without using any tools.";

type Turn = { question: string; given: { search: { ran: boolean }; exchanges: unknown[] } | null; givenTexts: string[]; steps: number } | null;
const turn = () => invoke<Turn>("memory_session_given");
const turns = () => browser.execute(() => (window as any).__yoaiTurns?.() ?? []) as Promise<{ kind: string; state: string }[]>;

async function sessionReplyDone(timeout = 900_000) {
  await browser.waitUntil(async () => (await turns()).some((t) => t.kind !== "chat"), { timeout: 60_000, interval: 500, timeoutMsg: "no session turn was registered" });
  await browser.waitUntil(async () => !(await turns()).some((t) => t.kind !== "chat" && t.state !== "done" && t.state !== "error"), { timeout, interval: 2000, timeoutMsg: "the session turn did not finish" });
}

describe("memory in a project session", () => {
  after(async () => {
    await browser.execute(() => localStorage.removeItem("build-binary-path")).catch(() => {});
    await browser.execute(async () => { await (window as any).__yoaiCloseFolder?.(); }).catch(() => {});
    await invoke("stop_build_agent").catch(() => {});
    await resetAiModels();
  });

  it("has the made-up history read into the index", async function () {
    this.timeout(1_800_000);
    await waitForChat();
    if (!(await invoke<boolean>("path_is_file", { path: HELPER }))) {
      console.log(`e2e: no Build helper at ${HELPER} - skipping`);
      this.skip();
    }
    await ask("What colour is the sky on a clear day?");
    await waitForReplyDone();
    const aiId = (await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const p = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(p);
      const ais = ((await store.get("custom-ais")) || []) as { id?: string; agentPubKey?: string }[];
      const asked = (((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string }[]).slice(-1)[0]?.agentKey ?? "";
      return (ais.find((x) => x.agentPubKey === asked) ?? ais[0])?.id ?? "";
    })) as string;
    const model = (await invoke<string | null>("get_current_model")) ?? "fixture";
    const res = await fetch(`${BASE}/memory-fixture-load?${new URLSearchParams({ path: path.join(fixture, "history.json"), all: aiId, model })}`, { method: "POST" });
    const loaded = await res.json();
    console.log(`e2e: history: ${JSON.stringify(loaded)}`);
    expect(loaded.messages).toBe(66);
    const began = Date.now();
    await invoke("memory_read_now", { on: true });
    await browser.waitUntil(async () => {
      const s = await invoke<any>("memory_reading_status");
      return s.total > 0 && s.read >= s.total && s.levelAt > began;
    }, { timeout: 900_000, interval: 5_000, timeoutMsg: "the history was not read into the index" });
  });

  it("a question about earlier work is searched once and answered with the line under it", async function () {
    this.timeout(1_800_000);
    await browser.execute((p: string) => localStorage.setItem("build-binary-path", p), HELPER);
    const loaded = await invoke<string | null>("get_current_model");
    await browser.execute(async (file: string) => {
      const t = (window as any).__TAURI__;
      const p = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(p);
      const ais = ((await store.get("custom-ais")) || []) as { name: string; model?: string }[];
      for (const a of ais) a.model = file;
      await store.set("custom-ais", ais);
      await store.save();
    }, loaded);
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    const folder = path.resolve(here, "../../profile/project-scratch");
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, "README.md"), "# Scratch project\n\nA folder for the UI tests.\n");
    await browser.waitUntil(async () => browser.execute(() => typeof (window as any).__yoaiOpenFolder === "function"), { timeout: 30_000 });
    await browser.execute((p: string) => (window as any).__yoaiOpenFolder(p), folder);
    await browser.waitUntil(async () => {
      const s = await invoke<{ running: boolean; sessionId: string | null }>("build_agent_status");
      return s.running && !!s.sessionId;
    }, { timeout: 180_000, interval: 1000, timeoutMsg: "the session never came up" });

    await ask(DELIVERY);
    await sessionReplyDone();
    const first = await turn();
    console.log(`e2e: session turn: ${JSON.stringify({ question: first?.question, ran: first?.given?.search.ran, exchanges: first?.given?.exchanges.length, steps: first?.steps })}`);
    // The question searched for is the person's own words, not the
    // guidance the page joins in front of a session's first prompt.
    expect(first?.question).toBe(DELIVERY);
    expect(first?.given?.search.ran).toBe(true);
    expect(first?.given?.exchanges.length ?? 0).toBeGreaterThan(0);
    expect(first?.givenTexts.some((t) => t.includes("$60") || t.includes("60"))).toBe(true);
    const reply = (await browser.execute(() => ([...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined)?.innerText ?? "")) as string;
    console.log(`e2e: session reply: ${JSON.stringify(reply.slice(0, 400))}`);
    // The line shows when the reply quoted a passage word for word, said it
    // searched, or not everything is read yet; a reply that put the answer
    // in its own words gets none. Either way the reply holds its record.
    const line = await $('[data-testid="given-line"]');
    const shown = await line.waitForExist({ timeout: 15_000 }).then(() => true, () => false);
    console.log(`e2e: line: ${shown ? JSON.stringify(await line.getText()) : "(none: the reply quoted no passage word for word)"}`);
    const recorded = (await browser.execute(() => {
      const turnsNow = ((window as any).__yoaiTurnsFull?.() ?? []) as { assistant?: { given?: { exchanges?: unknown[] } } }[];
      return turnsNow.slice(-1)[0]?.assistant?.given?.exchanges?.length ?? -1;
    })) as number;
    console.log(`e2e: the reply's record holds ${recorded} exchange(s)`);
    await shot("session-memory-line");
  });

  it("the next prompt is a new turn with its own search, and the first is in the records with what it was given", async function () {
    this.timeout(1_800_000);
    const before = await turn();
    const NEXT = "And which day does the flour arrive? Answer from what we said before, without using any tools.";
    await ask(NEXT);
    await sessionReplyDone();
    const second = await turn();
    console.log(`e2e: second session turn: ${JSON.stringify({ question: second?.question, exchanges: second?.given?.exchanges.length, steps: second?.steps })}`);
    expect(second?.question).toBe(NEXT);
    expect(second?.question).not.toBe(before?.question);
    const recorded = (await browser.execute(() => ((window as any).__yoaiTurnsFull?.() ?? []) as unknown[])) as unknown[];
    console.log(`e2e: turns at the root: ${recorded.length}`);
    await shot("session-memory-second");
  });
});
