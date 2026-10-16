// Shared helpers for the installed-models flows.
import { resolve } from "node:path";
import { SHOTS } from "../../wdio.conf";

export const shot = (name: string) => browser.saveScreenshot(resolve(SHOTS, `${name}.png`));

/** The composer is a contenteditable; the driver's key events do not reach
 *  its input handler, so type the way a paste would. */
export async function typeInto(testId: string, text: string) {
  await browser.execute((id: string, t: string) => {
    const el = document.querySelector(`[data-testid="${id}"]`) as HTMLElement;
    el.focus();
    el.textContent = t;
    el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: t }));
  }, testId, text);
}

/** The last assistant message: its state and text, or null. */
export const lastReply = () =>
  browser.execute(() => {
    const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
    return m ? { state: m.dataset.state, stopped: m.dataset.stopped === "1", text: m.innerText } : null;
  });

/** The chat page, with a model LOADED - asked of the engine, never read off
 *  a dropdown: a card with no room would otherwise fail every reply. */
export async function waitForChat() {
  // The app reopens on the page it was left on (the profile lives across
  // the specs of one run): go to the chat explicitly.
  if ((await browser.execute(() => location.pathname)) !== "/chat/") await browser.url("tauri://localhost/chat/");
  await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/chat/", { timeout: 60_000 });
  await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
  // The profile lives across the specs of one run: a reply a failed spec left
  // running would be reopened by the page (it runs on at the root) and its
  // Stop would stand where Send is. Stop it, and start from the start view.
  const stopped = await browser.execute(() => {
    const w = window as any;
    const live = (w.__yoaiTurns?.() ?? []).filter((t: { state: string }) => t.state === "running" || t.state === "queued" || t.state === "waiting");
    for (const t of live) w.__yoaiStopTurn?.(t.id);
    return live.length;
  });
  if (stopped) console.log(`e2e: stopped ${stopped} leftover turn(s)`);
  // A project left open by an earlier spec would take the next question
  // through the agent: close it.
  const closed = await browser.execute(async () => {
    const w = window as any;
    const s = await w.__TAURI__.core.invoke("build_agent_status").catch(() => null);
    if (s?.folder && typeof w.__yoaiCloseFolder === "function") { await w.__yoaiCloseFolder(); return s.folder; }
    return null;
  });
  if (closed) console.log(`e2e: closed leftover project ${closed}`);
  if (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length > 0)) {
    await browser.waitUntil(async () => browser.execute(() => !((window as any).__yoaiTurns?.() ?? []).some((t: { state: string }) => t.state === "running")), { timeout: 30_000, timeoutMsg: "a leftover reply would not stop" });
    await (await $('[data-testid="new-chat"]')).click();
    await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 15_000, timeoutMsg: "the new chat did not clear the page" });
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 30_000 });
  }
  let model: string | null = null;
  await browser.waitUntil(async () => { model = await invoke<string | null>("get_current_model"); return !!model; }, {
    timeout: 180_000,
    timeoutMsg: "no model loaded within 3 minutes (is the card's memory free?)",
  });
  console.log(`e2e: model loaded: ${model}`);
}

export async function ask(text: string) {
  await typeInto("chat-input", text);
  const send = await $('[data-testid="chat-send"]');
  await send.waitForEnabled({ timeout: 10_000 });
  await send.click();
}

export async function waitForReplyDone(timeout = 180_000) {
  await browser.waitUntil(async () => (await lastReply())?.state === "done", { timeout, timeoutMsg: "the assistant never finished" });
  return (await lastReply())!;
}

/** Specs share one profile in a suite run: a spec that pinned the AIs' models
 *  puts them back to automatic, so the next spec meets the AIs it expects. */
export async function resetAiModels() {
  try {
    await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = ((await store.get("custom-ais")) || []) as { model?: string }[];
      for (const a of ais) a.model = "auto:offline";
      await store.set("custom-ais", ais);
      await store.save();
    });
  } catch (e) {
    console.log(`e2e: could not reset the AIs' models: ${String(e).slice(0, 120)}`);
  }
}

/** Tauri's invoke from inside the page (withGlobalTauri). */
export const invoke = <T = unknown>(cmd: string, args: Record<string, unknown> = {}) =>
  browser.execute((c: string, a: Record<string, unknown>) => (window as any).__TAURI__.core.invoke(c, a) as Promise<T>, cmd, args) as Promise<T>;

/** Scroll an element to the middle of the view, instantly. WebdriverIO's own
 *  scrollIntoView waits on an async script that timed out (30 s per try) on
 *  the models page; a plain DOM scroll needs no frame. */
export async function scrollTo(sel: string) {
  await browser.execute((s: string) => document.querySelector(s)?.scrollIntoView({ block: "center", behavior: "instant" as ScrollBehavior }), sel);
}

/** Start `n` conversations on one AI the way the app writes them - in
 *  batches of 25 per browser call, so no call outlives the driver's script
 *  limit (one call for 1,000 timed out and was retried four times over). */
export async function seedConversations(agentKey: string, aiName: string, n: number): Promise<number> {
  let made = 0;
  for (let i = 0; i < n; i += 25) {
    made += await browser.execute(async (key: string, name: string, from: number, count: number) => {
      const call = (window as any).__TAURI__.core.invoke;
      let done = 0;
      for (let k = 0; k < count; k += 5) {
        await Promise.all(Array.from({ length: Math.min(5, count - k) }, (_, x) =>
          call("start_conversation", { agentKey: key, aiName: name, model: "e2e", title: `Seeded conversation ${from + k + x}`, source: null }).then(() => done++),
        ));
      }
      return done;
    }, agentKey, aiName, i, Math.min(25, n - i));
  }
  return made;
}

/** Run one truth-matrix leg to the end and hand back its lines. A WebDriver
 *  script call cannot wait minutes: fire it, watch its progress events. */
export async function runMatrixLeg(leg: string, timeoutMs = 1_800_000): Promise<{ lines: string[]; error?: string }> {
  const stamp = `${leg}-${Date.now().toString(36)}`;
  await browser.execute((st: string, l: string) => {
    const w = window as any;
    w.__matrixDone = false;
    w.__matrixLines = [];
    w.__matrixError = undefined;
    w.__TAURI__.event.listen("matrix-progress", (e: any) => {
      const line = String(e.payload ?? "");
      w.__matrixLines.push(line);
      if (line.includes("MATRIX COMPLETE")) w.__matrixDone = true;
    });
    w.__TAURI__.core.invoke("matrix_run", { stamp: st, legs: [l] }).then(() => { w.__matrixDone = true; }).catch((e: unknown) => { w.__matrixError = String(e); w.__matrixDone = true; });
  }, stamp, leg);
  await browser.waitUntil(async () => browser.execute(() => (window as any).__matrixDone === true), { timeout: timeoutMs, interval: 10_000, timeoutMsg: `the ${leg} leg never finished` });
  const lines = await browser.execute(() => (window as any).__matrixLines as string[]);
  const error = await browser.execute(() => (window as any).__matrixError as string | undefined);
  return { lines, error };
}

/** The routing battery's verdict from its lines: failures listed, and the
 *  time per decision (from "N decisions in X s"). */
export function routingVerdict(lines: string[]) {
  const failures = lines.filter((l) => /^\s*FAIL /.test(l) || /^routing: /.test(l.trim()));
  const timing = lines.map((l) => /\((\d+) decisions in (\d+) s\)/.exec(l)).find(Boolean);
  const decided = timing ? Number(timing[1]) : 0;
  const msPer = timing && decided ? (Number(timing[2]) * 1000) / decided : null;
  const complete = lines.find((l) => l.includes("MATRIX COMPLETE")) ?? "";
  return { failures, decided, msPer, complete };
}
