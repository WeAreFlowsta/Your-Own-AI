// An agent (project) turn keeps going when you leave the chat (the design
// notes, step 1b): a project is opened, a question is sent to
// the agent, the person opens Settings while it works, comes back - the turn is
// still on screen or finished, the page has taken it over, and it is in the
// records. Needs the Build helper installed on this machine (skipped otherwise).
import { homedir } from "node:os";
import { mkdirSync, writeFileSync, readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import { shot, waitForChat, invoke, ask, resetAiModels } from "./_helpers";

// YOAI_E2E_HELPER: another copy of the helper to drive (a new release, before the app installs it).
const HELPER = process.env.YOAI_E2E_HELPER || resolve(homedir(), ".local/share/com.solar.yourowai/build/your-own-ai-build");
const QUESTION = "Without using any tools, write about 150 words on why tea became popular in Britain, in plain paragraphs.";

const turns = () => browser.execute(() => (window as any).__yoaiTurns?.() ?? []) as Promise<{ kind: string; state: string; chars: number; conv: string | null }[]>;

describe("an agent turn keeps going when you leave the chat", () => {
  after(async () => {
    // The Build helper's path and the model pins are this spec's; the specs
    // after it expect Build absent (the Build offer line) and automatic models.
    await browser.execute(() => localStorage.removeItem("build-binary-path")).catch(() => {});
    await browser.execute(async () => { await (window as any).__yoaiCloseFolder?.(); }).catch(() => {});
    await invoke("stop_build_agent").catch(() => {});
    await resetAiModels();
  });

  it("runs on while Settings is open and is on the page and in the records on return", async function () {
    this.timeout(1_200_000);
    await waitForChat();
    if (!(await invoke<boolean>("path_is_file", { path: HELPER }))) {
      console.log(`e2e: no Build helper at ${HELPER} - skipping`);
      this.skip();
    }
    // The helper's path, as the installer records it; a scratch project folder.
    await browser.execute((p: string) => localStorage.setItem("build-binary-path", p), HELPER);
    // The agent serves on the AI's own model when the AI is pinned to a file;
    // unpinned it picks the catalog's coder, which the test box's card may not
    // hold (a 4 GB card: the 4B coder at a 32k window fails to load). Pin every
    // AI to the chat model already loaded, then reopen the chat so it reads it.
    const loaded = await invoke<string | null>("get_current_model");
    // YOAI_E2E_NO_PIN=1 leaves the AIs on automatic: the agent's own pick (a
    // 4B coder at a 32k window) does not fit this card - what happens then?
    const pinned = process.env.YOAI_E2E_NO_PIN ? ["(not pinned)"] : await browser.execute(async (file: string) => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = ((await store.get("custom-ais")) || []) as { name: string; model?: string }[];
      for (const a of ais) a.model = file;
      await store.set("custom-ais", ais);
      await store.save();
      return ais.map((a) => `${a.name}=${a.model}`);
    }, loaded);
    console.log(`e2e: AIs pinned ${JSON.stringify(pinned)}`);
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    const folder = resolve(import.meta.dirname ?? ".", "../../profile/project-scratch");
    mkdirSync(folder, { recursive: true });
    writeFileSync(resolve(folder, "README.md"), "# Scratch project\n\nA folder for the UI tests.\n");
    await browser.waitUntil(async () => browser.execute(() => typeof (window as any).__yoaiOpenFolder === "function"), { timeout: 30_000 });
    await browser.execute((p: string) => (window as any).__yoaiOpenFolder(p), folder);
    // The session is up when the bridge reports a session id.
    await browser.waitUntil(async () => {
      const s = await invoke<{ running: boolean; sessionId: string | null }>("build_agent_status");
      return s.running && !!s.sessionId;
    }, { timeout: 180_000, interval: 1000, timeoutMsg: "the agent session never came up" });
    console.log("e2e: agent session up");
    await shot("agent-continues-open");

    await ask(QUESTION);
    // The turn is registered at the root as an agent turn.
    try {
      await browser.waitUntil(async () => (await turns()).some((t) => t.kind !== "chat"), { timeout: 60_000, interval: 500, timeoutMsg: "no agent turn was registered at the root" });
    } catch (e) {
      // Say what the root held and what the page shows, so a miss can be read.
      console.log(`e2e: root turns at the miss: ${JSON.stringify(await turns())}`);
      console.log(`e2e: page at the miss: ${JSON.stringify(await browser.execute(() => ({ bubbles: document.querySelectorAll('[data-testid="chat-message"]').length, stalls: localStorage.getItem("uiStalls") })))}`);
      throw e;
    }
    // Some work has arrived (narration on the rail or text).
    await browser.waitUntil(async () => (await turns()).some((t) => t.kind !== "chat" && t.chars > 0) || (await browser.execute(() => {
      const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
      return !!m && m.innerText.length > 40;
    })), { timeout: 240_000, interval: 1000, timeoutMsg: "the agent never started working" });
    console.log(`e2e: turns while working ${JSON.stringify(await turns())}`);
    await shot("agent-continues-working");
    // What the page reports as going wrong from here on, for a miss to be read.
    await browser.execute(() => {
      const w = window as any;
      w.__e2eErrors = [];
      const note = (s: unknown) => w.__e2eErrors.push(String(s).slice(0, 300));
      window.addEventListener("error", (e) => note(`error: ${e.message} at ${e.filename}:${e.lineno}`));
      window.addEventListener("unhandledrejection", (e) => note(`rejection: ${(e as PromiseRejectionEvent).reason?.stack ?? (e as PromiseRejectionEvent).reason}`));
      const was = console.error.bind(console);
      console.error = (...a: unknown[]) => { note(`console.error: ${a.map((x) => (x instanceof Error ? x.stack : String(x))).join(" ")}`); was(...a); };
    });

    // Leave for Settings through the app's own menu.
    await (await $('button[title^="Menu"]')).click();
    await browser.execute(() => {
      const item = [...document.querySelectorAll('[role="menuitem"]')].find((b) => (b as HTMLElement).innerText.trim().startsWith("Settings")) as HTMLElement | undefined;
      item?.click();
    });
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/settings/", { timeout: 30_000, timeoutMsg: "Settings never opened" });
    await new Promise((r) => setTimeout(r, 6000));
    console.log(`e2e: turns while on Settings ${JSON.stringify(await turns())}`);

    // Back through the logo: the page opens the turn's conversation by itself.
    await browser.execute(() => {
      const img = [...document.querySelectorAll('img[alt="Your Own AI"]')].find((i) => (i as HTMLElement).offsetParent !== null) as HTMLElement | undefined;
      (img?.closest("button") as HTMLElement | null)?.click();
    });
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/chat/", { timeout: 30_000, timeoutMsg: "the chat never reopened" });
    await new Promise((r) => setTimeout(r, 4000));
    console.log(`e2e: turns on return ${JSON.stringify(await turns())}`);
    console.log(`e2e: the page on return: ${JSON.stringify(await browser.execute(() => ({
      messages: [...document.querySelectorAll('[data-testid="chat-message"]')].map((m) => `${m.getAttribute("data-role")}:${m.getAttribute("data-state") ?? ""}:${(m as HTMLElement).innerText.length}`),
      errors: (window as any).__e2eErrors ?? [],
    })))}`);
    // The turn is on screen: the agent bubble, still working or done.
    await browser.waitUntil(async () => browser.execute(() => {
      const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
      return !!m && m.innerText.length > 20;
    }), { timeout: 60_000, interval: 500, timeoutMsg: "the agent's bubble was not shown after coming back" });
    const bubbles = await browser.execute(() => ({
      unanswered: document.body.innerText.includes("never got an answer"),
      users: document.querySelectorAll('[data-testid="chat-message"][data-role="user"]').length,
      assistants: document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length,
    }));
    console.log(`e2e: bubbles on return ${JSON.stringify(bubbles)}`);
    expect(bubbles.unanswered).toBe(false);
    expect(bubbles.users).toBe(1);
    expect(bubbles.assistants).toBe(1);
    await shot("agent-continues-back");

    // Let it finish: the root turn ends and the bubble on the page is done.
    // (Progress every 20 s: the rail's item count and the bubble's text length.)
    let lastNote = 0;
    await browser.waitUntil(async () => {
      const ts = await turns();
      if (Date.now() - lastNote > 20_000) {
        lastNote = Date.now();
        const len = await browser.execute(() => ([...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined)?.innerText.length ?? 0);
        const status = await invoke<{ running: boolean; sessionId: string | null }>("build_agent_status").catch(() => null);
        console.log(`e2e: waiting - turns ${JSON.stringify(ts)} bubble ${len} chars agent ${JSON.stringify(status)}`);
      }
      // A small local model may reach for a tool though it was asked not to
      // (11-25: it asked to write a file, and the turn waited on the card
      // for ten minutes). Decline, as a person who asked for words would.
      const deny = await $('[data-testid="permission-deny"]');
      if (await deny.isExisting().catch(() => false)) {
        await deny.click().catch(() => {});
        console.log("e2e: declined a tool request");
      }
      return ts.every((t) => t.kind === "chat" || (t.state !== "running" && t.state !== "waiting"));
    }, { timeout: 600_000, interval: 1000, timeoutMsg: "the agent turn never finished" });
    await browser.waitUntil(async () => browser.execute(() => {
      const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
      return !!m && m.getAttribute("data-state") === "done";
    }), { timeout: 60_000, interval: 500, timeoutMsg: "the bubble never settled" });
    const done = await turns();
    console.log(`e2e: turns at the end ${JSON.stringify(done)}`);
    expect(done.some((t) => t.kind !== "chat" && t.state === "done")).toBe(true);
    const finalLen = await browser.execute(() => ([...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement).innerText.length);
    console.log(`e2e: finished, ${finalLen} chars on screen`);
    expect(finalLen).toBeGreaterThan(100);
    // Records: the conversation holds an assistant entry.
    const full = await browser.execute(() => {
      const w = window as any;
      return w.__yoaiTurnsFull ? w.__yoaiTurnsFull() : null;
    }) as { agentKey: string; conversationHash: string | null }[] | null;
    const t = full?.find((x) => x.conversationHash);
    if (t) {
      const entries = await invoke<{ role: string; content: string }[]>("get_conversation_transcript", { agentKey: t.agentKey, conversationHash: t.conversationHash }).catch((e) => { console.log(`e2e: transcript read said ${String(e).slice(0, 160)}`); return []; });
      const assistant = entries.filter((e) => e.role === "assistant");
      console.log(`e2e: records hold ${entries.length} entries, ${assistant.length} assistant, last ${assistant.at(-1)?.content.length ?? 0} chars`);
      expect(assistant.length).toBeGreaterThan(0);
    }
    await shot("agent-continues-done");
    // Leave nothing for the specs that follow: the project closed on the page
    // (its conversation would otherwise be resumed INTO the project), a fresh chat.
    await browser.execute(async () => { await (window as any).__yoaiCloseFolder?.(); });
    await (await $('[data-testid="new-chat"]')).click();
    await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 15_000 });
    await invoke("stop_build_agent").catch(() => {});
  });

  it("lets you allow an agent's ask from the tray while you are elsewhere", async function () {
    this.timeout(900_000);
    await waitForChat();
    if (!(await invoke<boolean>("path_is_file", { path: HELPER }))) this.skip();
    await browser.execute((p: string) => localStorage.setItem("build-binary-path", p), HELPER);
    const folder = resolve(import.meta.dirname ?? ".", "../../profile/project-scratch");
    mkdirSync(folder, { recursive: true });
    await browser.waitUntil(async () => browser.execute(() => typeof (window as any).__yoaiOpenFolder === "function"), { timeout: 30_000 });
    await browser.execute((p: string) => (window as any).__yoaiOpenFolder(p), folder);
    await browser.waitUntil(async () => {
      const s = await invoke<{ running: boolean; sessionId: string | null }>("build_agent_status");
      return s.running && !!s.sessionId;
    }, { timeout: 180_000, interval: 1000, timeoutMsg: "the agent session never came up" });
    await ask("Create a file named hello.txt in this folder containing the single word hello. Do nothing else.");
    // A write asks for permission (the session opens in Ask mode). A model that
    // never gets there within four minutes leaves nothing to test here.
    let asked = false;
    await browser.waitUntil(async () => {
      const ts = await turns();
      asked = ts.some((t) => t.kind === "project" && t.state === "waiting");
      return asked || ts.every((t) => t.kind !== "project" || (t.state !== "running"));
    }, { timeout: 240_000, interval: 1000 }).catch(() => {});
    if (!asked) {
      console.log("e2e: the agent did not ask for permission in time - skipping (model quality)");
      await browser.execute(async () => { await (window as any).__yoaiCloseFolder?.(); });
      this.skip();
    }
    // Away to Settings: the ask shows in the tray, with Allow.
    await (await $('button[title^="Menu"]')).click();
    await browser.execute(() => {
      const item = [...document.querySelectorAll('[role="menuitem"]')].find((b) => (b as HTMLElement).innerText.trim().startsWith("Settings")) as HTMLElement | undefined;
      item?.click();
    });
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/settings/", { timeout: 30_000 });
    const row = await $('[data-testid="turn-row"][data-turn-state="waiting"]');
    await row.waitForDisplayed({ timeout: 15_000 });
    const rowText = (await row.getText()).replace(/\s+/g, " ");
    console.log(`e2e: waiting row ${JSON.stringify(rowText.slice(0, 160))}`);
    expect(rowText).toContain("needs you");
    expect(rowText).toContain("Allow");
    await shot("agent-ask-in-tray");
    // The gated page read is allowed and seen through. Another tool the
    // model reached for is declined: the person was asked, which is what is
    // held here, and where that tool leads is not this step's matter
    // (11-26: the model asked to open the page with a browser tool, it was
    // allowed, and the turn ran past the time allowed).
    await browser.execute((label: string) => {
      const r = document.querySelector('[data-testid="turn-row"][data-turn-state="waiting"]');
      const b = [...(r?.querySelectorAll("button") ?? [])].find((x) => x.textContent?.trim() === label) as HTMLElement | undefined;
      b?.click();
    }, gatedRead ? "Allow" : "Don't allow");
    const asked = Date.now();
    await browser.waitUntil(async () => {
      const ts = await turns();
      const done = ts.every((t) => t.kind !== "tools" || (t.state !== "waiting" && t.state !== "running"));
      // After a decline the model may go on asking: two minutes, then the turn is stopped.
      if (!done && !gatedRead && Date.now() - asked > 120_000) {
        await browser.execute(() => {
          const w = window as any;
          for (const t of (w.__yoaiTurns?.() ?? []) as { id: string; state: string }[]) if (t.state === "running" || t.state === "waiting") w.__yoaiStopTurn?.(t.id);
        });
      }
      return done;
    }, { timeout: 600_000, interval: 1000, timeoutMsg: "the tools turn never finished after the answer" });
    const read = (() => { try { return readFileSync(appLog, "utf8").split("\n").filter((l) => /\[web\] .* reads https?:\/\//.test(l)); } catch { return []; } })();
    console.log(`e2e: pages read ${JSON.stringify(read.map((l) => l.replace(/^.*\[web\] /, "")).slice(-4))}`);
    // Two pages when the gated read was allowed; at least the person's own
    // address when the model took another road to the second.
    expect(read.length).toBeGreaterThanOrEqual(gatedRead ? 2 : 1);
    await browser.execute(() => {
      const img = [...document.querySelectorAll('img[alt="Your Own AI"]')].find((i) => (i as HTMLElement).offsetParent !== null) as HTMLElement | undefined;
      (img?.closest("button") as HTMLElement | null)?.click();
    });
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/chat/", { timeout: 30_000 });
    await browser.execute(async () => { await (window as any).__yoaiCloseFolder?.(); });
    await invoke("stop_build_agent").catch(() => {});
  });
});
