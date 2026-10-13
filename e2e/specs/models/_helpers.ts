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

/** Tauri's invoke from inside the page (withGlobalTauri). */
export const invoke = <T = unknown>(cmd: string, args: Record<string, unknown> = {}) =>
  browser.execute((c: string, a: Record<string, unknown>) => (window as any).__TAURI__.core.invoke(c, a) as Promise<T>, cmd, args) as Promise<T>;

/** Scroll an element to the middle of the view, instantly. WebdriverIO's own
 *  scrollIntoView waits on an async script that timed out (30 s per try) on
 *  the models page; a plain DOM scroll needs no frame. */
export async function scrollTo(sel: string) {
  await browser.execute((s: string) => document.querySelector(s)?.scrollIntoView({ block: "center", behavior: "instant" as ScrollBehavior }), sel);
}
