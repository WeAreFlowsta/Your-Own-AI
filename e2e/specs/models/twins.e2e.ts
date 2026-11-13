// A starter AI that stands twice (a household that added its second
// computer before a joining device's starters folded into the identity's)
// becomes one AI: one entry in the lists, both histories, one memory. The
// twin is planted here as its own AI under a setup name, given a
// conversation and a memory row, then renamed to the starter's name: from
// then on the two are the same AI to the person.
import { waitForChat, invoke, invokeLong } from "./_helpers";

const STAMP = Date.now().toString(36);
type Ai = Record<string, any>;

async function storeGet(path: string): Promise<Ai[]> {
  return browser.execute(async (p: string) => {
    const t = (window as any).__TAURI__.core;
    const rid = await t.invoke("plugin:store|load", { path: p, autoSave: false });
    const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
    const list = Array.isArray(got) && got.length === 2 && typeof got[1] === "boolean" ? got[0] : got;
    return Array.isArray(list) ? list : [];
  }, path) as unknown as Promise<Ai[]>;
}
async function storeSet(path: string, list: Ai[]) {
  await browser.execute(async (p: string, l: Ai[]) => {
    const t = (window as any).__TAURI__.core;
    const rid = await t.invoke("plugin:store|load", { path: p, autoSave: false });
    await t.invoke("plugin:store|set", { rid, key: "custom-ais", value: l });
    await t.invoke("plugin:store|save", { rid });
  }, path, list);
}
async function reloadToChat() {
  await browser.execute(() => location.reload());
  await browser.pause(2000);
  await waitForChat();
}
const row = (id: string, scope: string, text: string) => ({
  id, scope, kind: "note", text, as_of: Date.now(), saved_by: "person", provenance: "authored",
  pinned: false, topics: [], replaces: [], deleted: false, updated_at: Date.now(),
});
async function talk(agentKey: string, aiName: string, title: string): Promise<string> {
  const hash = await invoke<string>("start_conversation", { agentKey, aiName, model: "e2e", title, source: null });
  await invoke("record_transcript_entry", { agentKey, conversationHash: hash, role: "user", content: `${title}: hello`, sequence: 0, model: "e2e", thinking: null, tokens: null, provenance: null, agentLog: null, folderPath: null });
  return hash;
}

describe("a starter AI that stands twice", () => {
  it("becomes one AI with both histories and one memory", async function () {
    this.timeout(900_000);
    await waitForChat();
    const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
    let list = await storeGet(path);
    const starter = list.find((a) => a.agentPubKey && !a.foldedInto && ["veebo", "teresa", "reeves"].includes(a.baseArchetypeId) && (a.status ?? "active") === "active");
    expect(starter).toBeTruthy();
    const name: string = starter!.name;
    const setupName = `${name} setup ${STAMP}`;
    const K1: string = starter!.agentPubKey;
    const F1: string = starter!.foundingKey || K1;

    // The twin: the same starter, as its own AI for now.
    const { id: _i, agentPubKey: _a, foundingKey: _f, seedSlot: _s, updatedAt: _u, restoredFromId: _r, foldedInto: _o, ...same } = starter!;
    list.push({ ...same, id: `e2e-twin-${STAMP}`, name: setupName, status: "active" });
    await storeSet(path, list);
    await reloadToChat();
    let twin: Ai | undefined;
    await browser.waitUntil(async () => { twin = (await storeGet(path)).find((a) => a.name === setupName && a.agentPubKey); return !!twin; },
      { timeout: 300_000, interval: 3_000, timeoutMsg: "the planted AI was never given an agent of its own" });
    const K2: string = twin!.agentPubKey;
    const F2: string = twin!.foundingKey || K2;
    expect(K2).not.toBe(K1);
    console.log(`e2e: starter ${name} ${K1.slice(0, 12)} (founded ${F1.slice(0, 12)}), twin ${K2.slice(0, 12)}`);

    // Each has a conversation and a memory row of its own.
    const c1 = await talk(K1, name, `starter talk ${STAMP}`);
    const c2 = await talk(K2, name, `twin talk ${STAMP}`);
    await invokeLong("save_memory_row", { agentKey: K1, row: row(`e2e-starter-${STAMP}`, `ai:${F1}`, `the starter remembers ${STAMP}`) });
    await invokeLong("save_memory_row", { agentKey: K2, row: row(`e2e-twin-${STAMP}`, `ai:${F2}`, `the twin remembers ${STAMP}`) });
    // Two AIs so far: nothing folds.
    expect(await invoke<number>("fold_twins_now")).toBe(0);

    // A level chosen for tools in chat, for each (kept per computer, per AI):
    // the planted one "all", the starter "auto".
    const ids = await storeGet(path);
    const id1: string = ids.find((x) => x.agentPubKey === K1)!.id;
    const id2: string = ids.find((x) => x.agentPubKey === K2)!.id;
    const toolsBefore = await browser.execute((a: string, b: string) => {
      const before = localStorage.getItem("agentPermissionsByToolsAi");
      const map = before ? JSON.parse(before) : {};
      map[a] = "auto"; map[b] = "all";
      localStorage.setItem("agentPermissionsByToolsAi", JSON.stringify(map));
      return before;
    }, id1, id2);

    // Now they are the same AI to the person: the same starter, the same name.
    list = await storeGet(path);
    list.find((a) => a.agentPubKey === K2)!.name = name;
    await storeSet(path, list);
    expect(await invoke<number>("fold_twins_now")).toBeGreaterThanOrEqual(1);
    list = await storeGet(path);
    const a = list.find((x) => x.agentPubKey === K1)!;
    const b = list.find((x) => x.agentPubKey === K2)!;
    const [leader, folded] = F1 < F2 ? [a, b] : [b, a];
    const leaderFounding = F1 < F2 ? F1 : F2;
    expect(folded.foldedInto).toBe(leaderFounding);
    expect(leader.foldedInto).toBeUndefined();
    expect(await invoke<number>("fold_twins_now")).toBe(0);
    console.log(`e2e: ${leader.agentPubKey.slice(0, 12)} leads; ${folded.agentPubKey.slice(0, 12)} is folded into it and stays stored`);

    // The person sees one.
    await reloadToChat();
    await (await $('[data-testid="ai-selector"]')).click();
    await browser.pause(1000);
    const shown = await browser.execute((n: string) => [...document.querySelectorAll('[data-testid="ai-option"]')].filter((o) => (o as HTMLElement).dataset.aiLabel === n).length, name);
    const all = await browser.execute(() => document.querySelectorAll('[data-testid="ai-option"]').length);
    console.log(`e2e: the AI picker lists ${all} AI(s), ${shown} called ${name}`);
    expect(all).toBeGreaterThan(0);
    expect(shown).toBe(1);
    await browser.keys("Escape");

    // The level chosen for its tools in chat: the more cautious of the two,
    // on the one that leads, and the twin's own entry is gone.
    const tools = await browser.execute(() => JSON.parse(localStorage.getItem("agentPermissionsByToolsAi") ?? "{}") as Record<string, string>);
    console.log(`e2e: tools in chat ${JSON.stringify(tools)}`);
    expect(tools[leader.id]).toBe("auto");
    expect(tools[folded.id]).toBeUndefined();

    // Both histories under it, read through its own agent.
    const convs = await invokeLong<{ hash: string; title: string | null }[]>("get_conversations", { agentKey: leader.agentPubKey, maxAgeSecs: null, caller: "e2e twins" });
    const titles = convs.map((c) => c.title ?? "");
    expect(titles).toContain(`starter talk ${STAMP}`);
    expect(titles).toContain(`twin talk ${STAMP}`);
    // And it continues the twin's conversation.
    const other = leader.agentPubKey === K1 ? c2 : c1;
    await invoke("record_transcript_entry", { agentKey: leader.agentPubKey, conversationHash: other, role: "user", content: `continued by the one AI ${STAMP}`, sequence: 1, model: "e2e", thinking: null, tokens: null, provenance: null, agentLog: null, folderPath: null });

    // One memory.
    const rows = await invokeLong<{ text: string }[]>("list_memory_rows", { agentKey: leader.agentPubKey, scope: `ai:${leaderFounding}` });
    const texts = rows.map((r) => r.text);
    expect(texts).toContain(`the starter remembers ${STAMP}`);
    expect(texts).toContain(`the twin remembers ${STAMP}`);

    // Leave the profile as it was for the specs that follow: the planted AI
    // goes, and the mark with it.
    list = (await storeGet(path)).filter((x) => x.agentPubKey !== K2);
    await storeSet(path, list);
    await invoke<number>("fold_twins_now");
    await browser.execute((before: string | null) => { if (before === null) localStorage.removeItem("agentPermissionsByToolsAi"); else localStorage.setItem("agentPermissionsByToolsAi", before); }, toolsBefore);
    const after = await storeGet(path);
    expect(after.find((x) => x.agentPubKey === K1)!.foldedInto).toBeUndefined();
    await reloadToChat();
  });
});
