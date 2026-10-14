// Two AIs at once on one engine (build-docs ALWAYS_ON_AND_THREADS.md, step 2):
// Reeves answers a long question on one model; a new chat with Teresa, pinned to
// another model, asks and WAITS visibly for the engine ("Waiting for the engine ·
// Reeves is answering first") instead of restarting the server under Reeves.
// Meanwhile the activity tray shows both turns, the drawer marks Reeves's
// conversation live, and when Reeves finishes Teresa's turn runs. "Open" on the
// finished row brings Reeves's conversation back.
import { homedir } from "node:os";
import { resolve } from "node:path";
import { shot, waitForChat, invoke, ask } from "./_helpers";

const SECOND_MODEL = "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf";
const LONG = "Write about 2500 words on the history of tea across China, Japan, India, Russia, Morocco and Britain, in plain paragraphs with no headings. Take your time and cover each country in depth.";
const SHORT = "In one sentence, what is the capital of France?";

type T = { id: string; kind: string; state: string; chars: number; conv: string | null; ai: string };
const turns = () => browser.execute(() => (window as any).__yoaiTurns?.() ?? []) as Promise<T[]>;
const selectAi = async (label: string) => {
  await (await $('[data-testid="ai-selector"]')).click();
  const opt = await $(`[data-testid="ai-option"][data-ai-label="${label}"]`);
  await opt.waitForDisplayed({ timeout: 10_000 });
  await opt.click();
  await browser.waitUntil(async () => (await (await $('[data-testid="ai-selector"]')).getText()).includes(label), { timeout: 10_000, timeoutMsg: `${label} was not selected` });
};

describe("two AIs on one engine", () => {
  it("queues the second local reply visibly and lets both finish", async function () {
    this.timeout(1_200_000);
    await waitForChat();
    const second = resolve(homedir(), ".local/share/com.solar.yourowai/models", SECOND_MODEL);
    if (!(await invoke<boolean>("path_is_file", { path: second }))) {
      console.log(`e2e: no second model at ${second} - skipping`);
      this.skip();
    }
    // Reeves on the model already loaded, Teresa on the other one.
    const loaded = await invoke<string | null>("get_current_model");
    const pinned = await browser.execute(async (first: string, other: string) => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = ((await store.get("custom-ais")) || []) as { name: string; model?: string }[];
      for (const a of ais) a.model = a.name === "Teresa" ? other : first;
      await store.set("custom-ais", ais);
      await store.save();
      return ais.map((a) => `${a.name}=${a.model}`);
    }, loaded, SECOND_MODEL);
    console.log(`e2e: AIs pinned ${JSON.stringify(pinned)}`);
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    await selectAi("Reeves");

    await ask(LONG);
    // In a suite run the AI may carry documents from an earlier spec (a recall
    // step) and the window may grow for the long answer: allow for both.
    await browser.waitUntil(async () => (await turns()).some((t) => t.state === "running" && t.chars > 20), { timeout: 300_000, interval: 500, timeoutMsg: "Reeves never started streaming" });
    const reeves = (await turns()).find((t) => t.state === "running")!;
    console.log(`e2e: Reeves streaming ${JSON.stringify(reeves)}`);

    // A new chat lets go of Reeves's reply (it runs on at the root); Teresa asks.
    await (await $('[data-testid="new-chat"]')).click();
    await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 10_000, timeoutMsg: "the new chat did not clear the page" });
    // Straight to Teresa's question: every WebDriver step crawls while the
    // engine streams, and the queue only shows while Reeves is still answering.
    await selectAi("Teresa");
    await ask(SHORT);
    await browser.waitUntil(async () => (await turns()).some((t) => t.state === "queued"), { timeout: 60_000, interval: 500, timeoutMsg: "Teresa's turn never queued" });
    const page = await browser.execute(() => document.body.innerText.replace(/\s+/g, " "));
    console.log(`e2e: turns with Teresa queued ${JSON.stringify(await turns())}`);
    expect(page).toContain("Waiting for the engine");
    expect(page).toContain("Reeves is answering first");
    // Reeves's row is in the tray now that his conversation is off screen - read
    // together with the turns, in one snapshot: under load every step takes
    // seconds and Reeves may finish between two of them.
    const snap = await browser.execute(() => ({
      turns: (window as any).__yoaiTurns?.() ?? [],
      rows: [...document.querySelectorAll('[data-testid="turn-row"]')].map((r) => `${(r as HTMLElement).dataset.turnState}: ${(r as HTMLElement).innerText.replace(/\s+/g, " ").slice(0, 80)}`),
    })) as { turns: T[]; rows: string[] };
    console.log(`e2e: tray snapshot ${JSON.stringify(snap.rows)}`);
    const reevesStillRunning = snap.turns.some((t) => t.id === reeves.id && t.state === "running");
    if (reevesStillRunning) {
      expect(snap.rows.some((r) => r.startsWith("running:") && r.includes("Reeves is answering"))).toBe(true);
    } else {
      // He finished already: his row says so (for a minute) and Teresa's turn is on.
      expect(snap.rows.some((r) => r.includes("Reeves answered") || r.includes("Teresa is answering"))).toBe(true);
    }
    await shot("queue-waiting");

    // The drawer: Reeves's conversation carries the live dot; so does his chip.
    await (await $('[data-testid="conversations-open"]')).click();
    await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="conversation-item"]').length > 0), { timeout: 60_000, timeoutMsg: "the drawer never listed conversations" });
    // Read the dots and the turns in ONE snapshot: Reeves may have finished
    // between two slow steps, and then no dot is right.
    const drawer = await browser.execute(() => ({
      live: document.querySelectorAll('[data-testid="conversation-item"][data-live="1"]').length,
      items: document.querySelectorAll('[data-testid="conversation-item"]').length,
      liveTurns: ((window as any).__yoaiTurns?.() ?? []).filter((t: { state: string; conv: string | null }) => (t.state === "running" || t.state === "queued") && t.conv).length,
    })) as { live: number; items: number; liveTurns: number };
    console.log(`e2e: drawer ${JSON.stringify(drawer)}`);
    expect(drawer.items).toBeGreaterThan(0);
    if (drawer.liveTurns > 0) expect(drawer.live).toBeGreaterThan(0);
    else expect(drawer.live).toBe(0);
    await shot("queue-drawer");
    await (await $('button[title="Close"]')).click();

    // Reeves finishes; Teresa's turn then runs (the engine switches model) and finishes.
    let lastNote = 0;
    await browser.waitUntil(async () => {
      const ts = await turns();
      if (Date.now() - lastNote > 20_000) { lastNote = Date.now(); console.log(`e2e: waiting ${JSON.stringify(ts)}`); }
      return ts.length >= 2 && ts.every((t) => t.state === "done");
    }, { timeout: 600_000, interval: 1000, timeoutMsg: "the two turns never both finished" });
    const done = await turns();
    console.log(`e2e: both done ${JSON.stringify(done)}`);
    const teresa = done.find((t) => t.id !== reeves.id)!;
    expect(teresa.chars).toBeGreaterThan(10);
    expect(done.find((t) => t.id === reeves.id)!.chars).toBeGreaterThan(200);
    await shot("queue-done");

    // Reeves's finished row: "Reeves answered · Open" opens his conversation.
    const doneRow = await $('[data-testid="turn-row"][data-turn-state="done"]');
    await doneRow.waitForDisplayed({ timeout: 10_000 });
    expect(await doneRow.getText()).toContain("Reeves answered");
    await browser.execute(() => {
      const r = document.querySelector('[data-testid="turn-row"][data-turn-state="done"]');
      const open = [...(r?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.trim() === "Open") as HTMLElement | undefined;
      open?.click();
    });
    await browser.waitUntil(async () => browser.execute((q: string) => document.body.innerText.includes(q), LONG.slice(0, 40)), { timeout: 60_000, timeoutMsg: "Open did not bring Reeves's conversation back" });
    const opened = await browser.execute(() => ({
      users: document.querySelectorAll('[data-testid="chat-message"][data-role="user"]').length,
      lastLen: ([...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined)?.innerText.length ?? 0,
    }));
    console.log(`e2e: opened ${JSON.stringify(opened)}`);
    expect(opened.lastLen).toBeGreaterThan(200);
    await shot("queue-opened");
  });
});
