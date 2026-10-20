// Projects and project memory: how long things take and what the person
// sees (Eric 10-17: "the best experience possible for devs using yoai").
// The 10-16 evening session is the reason: a project note took five minutes
// to save and the memory seven to read back, because every save and read
// walked every AI's whole conversation list, and a slow read started a
// second memory conversation (one folder had 18 on one AI).
//
// Every timed step is checked against a budget and written to
// e2e/shots/projects-timings.json, so a run is a measurement as well as a
// pass. The agent legs need the Build helper on this machine (skipped
// otherwise); the memory legs need only the records.
import { homedir } from "node:os";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { SHOTS, appUrl } from "../../wdio.conf";
import { shot, invoke, waitForChat, ask, lastReply, resetAiModels, seedConversations } from "./_helpers";

const RUN = Date.now().toString(36);
const FOLDER = resolve(import.meta.dirname ?? ".", `../../profile/project-${RUN}`);
const DEPLOY = `make ship-${RUN}`;
const HELPER = resolve(homedir(), ".local/share/com.solar.yourowai/build/your-own-ai-build");

const timings: Record<string, { ms: number; budget: number }> = {};
async function timed<T>(name: string, budget: number, step: () => Promise<T>): Promise<T> {
  const t0 = Date.now();
  const out = await step();
  const ms = Date.now() - t0;
  timings[name] = { ms, budget };
  console.log(`e2e: ${name} ${ms} ms (budget ${budget})`);
  expect(ms).toBeLessThan(budget);
  return out;
}

type Writer = { key: string; name: string };
const writers = () =>
  browser.execute(async () => {
    const t = (window as any).__TAURI__;
    const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
    const store = await t.store.load(path);
    const ais = ((await store.get("custom-ais")) || []) as { agentPubKey?: string; name: string }[];
    return ais.filter((a) => a.agentPubKey).map((a) => ({ key: a.agentPubKey!, name: a.name }));
  }) as Promise<Writer[]>;

/** The memory conversations the index names for a folder: [agent, hash]. */
async function indexed(folder: string): Promise<[string, string][]> {
  const path = await invoke<string>("profile_store_path", { name: "project-memory-index.json" });
  const file = await invoke<{ content?: string }>("read_file_for_context", { filePath: path }).catch(() => ({ content: "" }));
  const all = JSON.parse(file.content || "{}") as Record<string, [string, string][]>;
  return all[folder] ?? [];
}
const perWriter = (rows: [string, string][]) =>
  rows.reduce<Record<string, number>>((m, [a]) => ((m[a.slice(0, 12)] = (m[a.slice(0, 12)] ?? 0) + 1), m), {});

const memory = (fresh = false) =>
  invoke<{ content: string; updatedAt: number; revisions: number }>("project_memory_get", { folder: FOLDER, fresh });

/** The agent's own tools, called the way the agent calls them. */
async function mcp(name: string, args: Record<string, unknown>, w: Writer) {
  const r = await fetch("http://127.0.0.1:11435/mcp", {
    method: "POST",
    headers: { "content-type": "application/json", "x-project": FOLDER, "x-agent-key": w.key, "x-agent-label": w.name },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/call", params: { name, arguments: args } }),
  });
  const body = await r.json();
  return { text: String(body?.result?.content?.[0]?.text ?? ""), isError: !!body?.result?.isError };
}

async function waitFor(what: string, timeout: number, check: () => Promise<boolean>) {
  await browser.waitUntil(check, { timeout, interval: 500, timeoutMsg: what });
}

describe("projects: memory saves and reads in seconds, one memory conversation per AI", () => {
  let a: Writer;
  let b: Writer | undefined;

  before(async () => {
    mkdirSync(FOLDER, { recursive: true });
    writeFileSync(resolve(FOLDER, "README.md"), "# Test project\n\nA folder for the project-memory tests.\n");
    await waitForChat();
    await waitFor("no AI was provisioned", 180_000, async () => (await writers()).length > 0);
    [a, b] = await writers();
    console.log(`e2e: writers ${a.name}${b ? ` and ${b.name}` : ""}; folder ${FOLDER}`);
  });

  after(() => {
    writeFileSync(resolve(SHOTS, "projects-timings.json"), JSON.stringify({ run: RUN, at: new Date().toISOString(), timings }, null, 2));
  });

  it("saves revisions on one memory conversation per AI and reads them back at once", async function () {
    this.timeout(300_000);
    await timed("save: first revision (starts the conversation)", 15_000, () =>
      invoke("project_memory_save", { writerKey: a.key, writerLabel: a.name, folder: FOLDER, content: "- first revision" }),
    );
    await timed("save: second revision", 10_000, () =>
      invoke("project_memory_save", { writerKey: a.key, writerLabel: a.name, folder: FOLDER, content: `- second revision\n- Deploys run with \`${DEPLOY}\`` }),
    );
    const quick = await timed("read: from the copy", 1_000, () => memory());
    expect(quick.content).toContain(DEPLOY);
    const fresh = await timed("read: from the records", 10_000, () => memory(true));
    expect(fresh.content).toContain(DEPLOY);
    expect(fresh.revisions).toBe(2);
    expect(fresh.updatedAt).toBeGreaterThan(0);
    const rows = await indexed(FOLDER);
    console.log(`e2e: index ${JSON.stringify(perWriter(rows))}`);
    expect(rows.length).toBe(1);
  });

  it("takes the agent's note at once and the Remember button's, on the same conversation", async function () {
    this.timeout(300_000);
    const answer = await timed("agent note: the tool answers", 2_000, () => mcp("remember_for_project", { note: "Tests run with `npm test`." }, a));
    expect(answer.isError).toBe(false);
    await timed("agent note: in the memory", 20_000, () =>
      waitFor("the agent's note never reached the memory", 20_000, async () => (await memory()).content.includes("npm test")),
    );
    await timed("Remember button: note saved", 10_000, () =>
      invoke("project_memory_note", { writerKey: a.key, writerLabel: a.name, folder: FOLDER, note: "The staging site is wetest." }),
    );
    const now = await memory();
    expect(now.content).toContain("[saved by you] The staging site is wetest.");
    expect(now.content).toContain(`[saved by ${a.name}] Tests run with`);
    const read = await timed("agent read: read_project_memory", 2_000, () => mcp("read_project_memory", {}, a));
    expect(read.text).toContain("wetest");
    expect((await indexed(FOLDER)).length).toBe(1);
  });

  it("gives a second AI its own conversation, and only one", async function () {
    if (!b) this.skip();
    this.timeout(120_000);
    await timed("save: a second AI's first revision", 15_000, () =>
      invoke("project_memory_save", { writerKey: b!.key, writerLabel: b!.name, folder: FOLDER, content: `- Deploys run with \`${DEPLOY}\`\n- revised by the second AI` }),
    );
    await invoke("project_memory_save", { writerKey: b!.key, writerLabel: b!.name, folder: FOLDER, content: `- Deploys run with \`${DEPLOY}\`\n- revised again` });
    const rows = await indexed(FOLDER);
    console.log(`e2e: index ${JSON.stringify(perWriter(rows))}`);
    expect(Object.values(perWriter(rows))).toEqual([1, 1]);
    expect((await memory(true)).content).toContain("revised again");
  });

});

describe("projects: the agent in a project", () => {
  let haveHelper = false;

  before(async () => {
    haveHelper = await invoke<boolean>("path_is_file", { path: HELPER }).catch(() => false);
    if (!haveHelper) return;
    await browser.execute((p: string) => localStorage.setItem("build-binary-path", p), HELPER);
    // The agent serves on the AI's own model when the AI is pinned to a file
    // (a 4 GB card cannot hold the catalog coder at an agent window).
    const loaded = await invoke<string | null>("get_current_model");
    await browser.execute(async (file: string) => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = ((await store.get("custom-ais")) || []) as { model?: string }[];
      for (const x of ais) x.model = file;
      await store.set("custom-ais", ais);
      await store.save();
    }, loaded);
    await browser.url(appUrl("/chat/"));
    await waitForChat();
  });

  after(async () => {
    await browser.execute(() => localStorage.removeItem("build-binary-path")).catch(() => {});
    await browser.execute(async () => { await (window as any).__yoaiCloseFolder?.(); }).catch(() => {});
    await invoke("stop_build_agent").catch(() => {});
    await resetAiModels();
    writeFileSync(resolve(SHOTS, "projects-timings.json"), JSON.stringify({ run: RUN, at: new Date().toISOString(), timings }, null, 2));
  });

  async function openProject(label: string) {
    await browser.waitUntil(async () => browser.execute(() => typeof (window as any).__yoaiOpenFolder === "function"), { timeout: 30_000 });
    await timed(`open project: session up (${label})`, 120_000, async () => {
      await browser.execute((p: string) => (window as any).__yoaiOpenFolder(p), FOLDER);
      await waitFor("the agent session never came up", 120_000, async () => {
        const s = await invoke<{ running: boolean; sessionId: string | null }>("build_agent_status");
        return s.running && !!s.sessionId;
      });
    });
  }

  it("opens the project and shows its memory in the dialog", async function () {
    if (!haveHelper) this.skip();
    this.timeout(300_000);
    await openProject("first");
    await shot("projects-open");
    await (await $('[data-testid="project-pill"]')).click();
    await (await $('[data-testid="open-project-memory"]')).click();
    const dialog = await $('[data-testid="project-memory-dialog"]');
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await shot("projects-memory-dialog-opening");
    await timed("dialog: notes and count shown", 10_000, () =>
      waitFor("the project memory dialog never finished reading", 10_000, async () => (await dialog.getAttribute("data-loading")) === "0"),
    );
    const status = await (await $('[data-testid="project-memory-status"]')).getText();
    console.log(`e2e: dialog status "${status}"`);
    expect(status).toMatch(/revisions? in your records/);
    await shot("projects-memory-dialog");
    // The backdrop closes it.
    await browser.execute(() => (document.querySelector('[data-testid="project-memory-dialog"] > div') as HTMLElement | null)?.click());
  });

  it("knows the project's memory in a new session", async function () {
    if (!haveHelper) this.skip();
    this.timeout(900_000);
    // The run's 8 GB cap holds the chat model at an agent window, the app
    // and the window - not the helper model besides (three runs went over).
    // The reply does not need it; anything that does starts it again.
    await invoke("stop_utility_server").catch(() => {});
    await ask("Without using any tools: what command runs deploys in this project? Answer with the command only.");
    // A small local model may still reach for a tool (10-17: it asked to run
    // `find`, and the turn waited on the card). Decline, as a person who
    // wants the answer from memory would; the agent then answers.
    let declined = 0;
    const reply = await timed("agent: first reply in the project", 600_000, async () => {
      await browser.waitUntil(async () => {
        const deny = await $('[data-testid="permission-deny"]');
        if (await deny.isExisting().catch(() => false)) {
          await deny.click().catch(() => {});
          declined++;
        }
        return (await lastReply())?.state === "done";
      }, { timeout: 600_000, interval: 1_000, timeoutMsg: "the agent never finished" });
      return (await lastReply())!;
    });
    if (declined) console.log(`e2e: declined ${declined} tool request(s)`);
    console.log(`e2e: reply "${reply.text.slice(0, 200)}"`);
    await shot("projects-agent-reply");
    expect(reply.text).toContain(DEPLOY);
  });

  it("writes the session's summary on close without a second memory conversation or losing a note", async function () {
    if (!haveHelper) this.skip();
    this.timeout(600_000);
    // The agent saves a note mid-session; the summary must revise what the
    // memory says now, not what it said when the session opened.
    const [w0] = await writers();
    const LINT = `pnpm lint-${RUN}`;
    const saved = await mcp("remember_for_project", { note: `Lint with \`${LINT}\`.` }, w0);
    expect(saved.isError).toBe(false);
    await waitFor("the mid-session note never reached the memory", 20_000, async () => (await memory()).content.includes(LINT));
    const before = (await memory(true)).revisions;
    const rowsBefore = await indexed(FOLDER);
    const t0 = Date.now();
    await browser.execute(async () => { await (window as any).__yoaiCloseFolder?.(); });
    // The summary is a model call, then one save; a session with nothing
    // durable to add may leave the memory as it was - both are fine, a
    // second conversation for the same AI is not.
    const wrote = await browser
      .waitUntil(async () => (await memory(true)).revisions > before, { timeout: 240_000, interval: 3_000 })
      .then(() => true, () => false);
    const after = await memory(true);
    if (wrote) timings["close: summary written"] = { ms: Date.now() - t0, budget: 240_000 };
    console.log(`e2e: revisions ${before} -> ${after.revisions}${wrote ? ` in ${Date.now() - t0} ms` : " (nothing new to keep)"}`);
    const rowsAfter = await indexed(FOLDER);
    console.log(`e2e: index before ${JSON.stringify(perWriter(rowsBefore))} after ${JSON.stringify(perWriter(rowsAfter))}`);
    for (const n of Object.values(perWriter(rowsAfter))) expect(n).toBe(1);
    expect(after.content).toContain(LINT);
  });
});

// Last: these legs unload the models (they need none) and leave a big AI
// behind. With the big AI in place, the agent legs above went over the
// run's 8 GB cap; before it, they fit.
/** A finished live read answers callers for 15 s (conversation_cache::LIVE_REUSE). */
const REUSE_MS = 16_000;
const BIG = Number(process.env.YOAI_E2E_BIG_N ?? 1000);

describe(`projects at the size of a well-used AI (${BIG} conversations)`, () => {
  let w: Writer;
  const folder = `${FOLDER}-big`;

  before(async function () {
    this.timeout(1_800_000);
    mkdirSync(folder, { recursive: true });
    await waitForChat();
    w = (await writers())[0];
    // These legs need no model: free the chat and helper models' memory
    // (the run's 8 GB cap stopped a first attempt with both loaded).
    await invoke("stop_llama_server").catch(() => {});
    await invoke("stop_utility_server").catch(() => {});
    // Veebo had 1,071 conversations on 10-16; every save and read walked
    // them. Give one AI as many, the way the app writes them.
    const t0 = Date.now();
    const made = await seedConversations(w.key, w.name, BIG);
    timings[`seed: ${BIG} conversations`] = { ms: Date.now() - t0, budget: 1_800_000 };
    console.log(`e2e: seeded ${made} conversations on ${w.name} in ${Date.now() - t0} ms`);
  });

  after(() => {
    writeFileSync(resolve(SHOTS, "projects-timings.json"), JSON.stringify({ run: RUN, at: new Date().toISOString(), timings }, null, 2));
  });

  it("answers many reads of a big AI's list with one read", async function () {
    this.timeout(900_000);
    await browser.pause(REUSE_MS);
    const one = await timed("big list: one live read", 300_000, () =>
      invoke<unknown[]>("get_conversations", { agentKey: w.key, maxAgeSecs: null, caller: "e2e one" }),
    );
    expect(one.length).toBeGreaterThanOrEqual(BIG);
    await browser.pause(REUSE_MS);
    const single = timings["big list: one live read"].ms;
    // Shared: about one read plus handing the list back eight times. One
    // after another would be eight reads.
    const many = await timed("big list: eight at once", Math.max(3_000, single * 2.5), () =>
      Promise.all(Array.from({ length: 8 }, (_, i) => invoke<unknown[]>("get_conversations", { agentKey: w.key, maxAgeSecs: null, caller: `e2e burst ${i}` }))),
    );
    for (const m of many) expect(m.length).toBe(one.length);
  });

  it("saves and reads a project note in seconds on that AI", async function () {
    this.timeout(600_000);
    // The first save for a folder the index has never seen reads the list once.
    await timed("big: first save (one list read)", 120_000, () =>
      invoke("project_memory_save", { writerKey: w.key, writerLabel: w.name, folder, content: `- Deploys run with \`${DEPLOY}\`` }),
    );
    await timed("big: next save (index, no list read)", 5_000, () =>
      invoke("project_memory_save", { writerKey: w.key, writerLabel: w.name, folder, content: `- Deploys run with \`${DEPLOY}\`\n- second` }),
    );
    const note = await fetch("http://127.0.0.1:11435/mcp", {
      method: "POST",
      headers: { "content-type": "application/json", "x-project": folder, "x-agent-key": w.key, "x-agent-label": w.name },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/call", params: { name: "remember_for_project", arguments: { note: "Big-AI note." } } }),
    }).then((r) => r.json());
    expect(note?.result?.isError).toBe(false);
    await timed("big: agent note in the memory", 10_000, () =>
      waitFor("the note never reached the memory", 10_000, async () =>
        (await invoke<{ content: string }>("project_memory_get", { folder, fresh: false })).content.includes("Big-AI note."),
      ),
    );
    const fresh = await timed("big: read from the records", 10_000, () =>
      invoke<{ revisions: number }>("project_memory_get", { folder, fresh: true }),
    );
    expect(fresh.revisions).toBe(3);
    expect((await indexed(folder)).length).toBe(1);
  });
});
