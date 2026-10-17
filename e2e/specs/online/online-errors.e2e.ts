// When an online model fails, the reply says so in plain words - never raw
// JSON - and its Retry button works once the provider is back. The staging
// proxy fails a request on demand (x-yoai-test-fail, staging only); the test
// build of the app attaches it while a failure is set.
//
// Before 2026-10-17 a busy provider showed `Error: {"message":"Upstream
// provider error (429)"}` in the bubble, and a provider that failed after
// the reply had started left a cut-off reply that looked finished.
import { invoke, waitForChat, ask, lastReply, shot, resetAiModels } from "../models/_helpers";
import { haveIdentity, setAiModel, signInAsTestIdentity, ONLINE_MODEL } from "./_online";

const CASES: { mode: string; says: RegExp }[] = [
  { mode: "busy", says: /busy right now/ },
  { mode: "timeout", says: /didn't answer in time/ },
  { mode: "error", says: /had an error/ },
  { mode: "rejected", says: /refused this request/ },
  { mode: "stream", says: /stopped partway/ },
];

const replyError = () =>
  browser.execute(() => {
    const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
    // Not "error": the test driver reads a returned object with an `error`
    // field as a failed command.
    return m ? { failed: m.dataset.error === "1", text: m.querySelector('[data-testid="reply-error"]')?.textContent ?? "", retry: !!m.querySelector('[data-testid="reply-retry"]') } : null;
  });

describe("online errors, said plainly", () => {
  let ready = false;

  before(async function () {
    this.timeout(300_000);
    if (!haveIdentity()) return;
    await waitForChat();
    const session = await signInAsTestIdentity();
    expect(session.tier).toBe("pro");
    const key = (await browser.execute(async () => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      return (((await store.get("custom-ais")) || []) as { agentPubKey?: string }[]).find((a) => a.agentPubKey)?.agentPubKey ?? "";
    })) as string;
    // Pinned to one online model: a failure is that model's, with no routing in between.
    await setAiModel(key, `online:${ONLINE_MODEL}`);
    // Nothing here runs on the device: free the local models' memory (the
    // run's 8 GB cap stopped a run with them loaded).
    await invoke("stop_llama_server").catch(() => {});
    await invoke("stop_utility_server").catch(() => {});
    await browser.url("tauri://localhost/chat/");
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    ready = true;
  });

  after(async () => {
    await invoke("e2e_set_online_failure", { mode: null }).catch(() => {});
    await resetAiModels();
  });

  for (const c of CASES) {
    it(`a ${c.mode} failure reads as one plain sentence with a Retry that works`, async function () {
      if (!ready) this.skip();
      this.timeout(180_000);
      await invoke("e2e_set_online_failure", { mode: c.mode });
      await ask("Reply with the single word: ok");
      await browser.waitUntil(async () => (await replyError())?.failed === true, { timeout: 60_000, timeoutMsg: `the ${c.mode} failure never showed as an error` });
      const shown = (await replyError())!;
      console.log(`e2e: ${c.mode} -> "${shown.text}" (retry ${shown.retry})`);
      await shot(`online-error-${c.mode}`);
      expect(shown.text).toMatch(c.says);
      expect(shown.text).not.toContain("{");
      expect(shown.retry).toBe(true);
      // The provider is back: Retry answers.
      await invoke("e2e_set_online_failure", { mode: null });
      await (await $('[data-testid="reply-retry"]')).click();
      await browser.waitUntil(async () => {
        const r = await lastReply();
        const e = await replyError();
        return r?.state === "done" && e?.failed === false && r.text.trim().length > 0;
      }, { timeout: 90_000, timeoutMsg: `Retry after the ${c.mode} failure never answered` });
    });
  }
});
