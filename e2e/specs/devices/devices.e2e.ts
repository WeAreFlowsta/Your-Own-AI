// One identity on two devices, live (the design notes §5.4):
// device A renames an AI and backs up; its Vault copies the manifest to
// the identity's second Vault; device B signs in fresh there and shows the
// renamed AI with its own agent. Phase c: BOTH devices online at once (A
// unattended on the default ports, B driven on shifted ports - YOAI_PORT_OFFSET)
// on the staging rendezvous: A's conversation, a read-only copy in phase b,
// ARRIVES in B's own records and the composer opens. Three launches, one
// phase each (DEVICES_PHASE), each against its own TEST Vault
// (YOAI_VAULT_PORT) - never the installed one. Run: e2e/devices-live.sh
import { shot, waitForChat, invoke, ask, waitForReplyDone } from "../models/_helpers";

const MESSAGE = "Devices test: name one colour and nothing else.";
const MESSAGE_C = "Devices test two: name one animal and nothing else.";

const PHASE = process.env.DEVICES_PHASE ?? "";
const NAME = process.env.DEVICES_NAME ?? "Synced AI";
const RULE = `Always answer in one short sentence (devices ${process.env.DEVICES_NAME ?? ""})`.trim();
const AI_NOTE = `The office opens at nine (devices ${process.env.DEVICES_NAME ?? ""})`.trim();

/** This device's AI list from its store: name, agent, founding key. */
async function aiList() {
  return browser.execute(async (path: string) => {
    const t = (window as any).__TAURI__.core;
    const rid = await t.invoke("plugin:store|load", { path, autoSave: false });
    const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
    const list = Array.isArray(got) ? got[0] : got;
    return (Array.isArray(list) ? list : []).map((a: any) => ({ name: a.name, agent: a.agentPubKey ?? "", founding: a.foundingKey ?? a.agentPubKey ?? "" }));
  }, await invoke<string>("profile_store_path", { name: "ai-data.json" })) as Promise<{ name: string; agent: string; founding: string }[]>;
}

const escrow = () => invoke<{ state: string }>("vault_escrow_sync");
const bodyText = () => browser.execute(() => document.body.innerText);

async function yourAis() {
  await browser.url("tauri://localhost/your-ais/");
  await browser.waitUntil(async () => (await $$('button[title^="Edit AI"]')).length > 0, { timeout: 60_000, timeoutMsg: "the Your AIs page never listed an AI" });
}

describe(`devices, phase ${PHASE}`, () => {
  it(PHASE === "a" ? "A: renames an AI through the form and backs up" : PHASE === "c" ? "C: with A online too, A's conversation arrives in B's own records and the composer opens" : "B: signs in fresh and shows A's edit with its own agent", async () => {
    if (PHASE === "a") {
      await waitForChat();
      const st = await escrow();
      console.log(`e2e: escrow ${JSON.stringify(st)}`);
      expect(st.state).toBe("synced");
      await yourAis();
      const before = await bodyText();
      await (await $$('button[title^="Edit AI"]'))[0].click();
      const nameInput = await $('input[placeholder="E.g., Marketing Guru Bot"]');
      await nameInput.waitForDisplayed({ timeout: 15_000 });
      const oldName = await nameInput.getValue();
      await browser.execute((t: string) => {
        const el = document.querySelector('input[placeholder="E.g., Marketing Guru Bot"]') as HTMLInputElement;
        el.focus();
        el.value = t;
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      }, NAME);
      await shot("devices-a-edit");
      await (await $("button[type=submit]")).click();
      await browser.waitUntil(async () => (await bodyText()).includes(NAME), { timeout: 30_000, timeoutMsg: "the renamed AI never showed on Your AIs" });
      console.log(`e2e: renamed ${JSON.stringify(oldName)} -> ${JSON.stringify(NAME)} (page had ${before.includes(oldName) ? "" : "NOT "}listed it)`);
      await shot("devices-a-renamed");
      // A standing rule, kept here: a person-scope memory row. The live read
      // puts it in this device's rows cache, which rides the manifest, so the
      // other device has it while this one is off (the memory design §10).
      await invoke("save_memory_row", { agentKey: null, row: {
        id: `devices-${Date.now().toString(36)}`, scope: "person", kind: "rule", text: RULE, structured: null,
        as_of: Date.now(), valid_to: null, saved_by: "person", provenance: "authored", pinned: true, topics: [], replaces: [], deleted: false, updated_at: Date.now(),
      } });
      const rowsA = await invoke<{ text: string }[]>("list_memory_rows", { agentKey: null, scope: "person" });
      expect(rowsA.some((r) => r.text === RULE)).toBe(true);
      // And a note the renamed AI knows: an ai-scope row by its founding key,
      // which is the same address on the other device.
      const renamedA = (await aiList()).find((a) => a.name === NAME);
      expect(renamedA?.founding).toBeTruthy();
      await invoke("save_memory_row", { agentKey: renamedA!.agent, row: {
        id: `devices-ai-${Date.now().toString(36)}`, scope: `ai:${renamedA!.founding}`, kind: "note", text: AI_NOTE, structured: null,
        as_of: Date.now(), valid_to: null, saved_by: "person", provenance: "authored", pinned: false, topics: [], replaces: [], deleted: false, updated_at: Date.now(),
      } });
      const aiRowsA = await invoke<{ text: string }[]>("list_memory_rows", { agentKey: renamedA!.agent, scope: `ai:${renamedA!.founding}` });
      expect(aiRowsA.some((r) => r.text === AI_NOTE)).toBe(true);
      // One conversation on this device, so the other device can show it
      // from the Vault's copy while this one is off (MULTI_DEVICE.md §3.3).
      await waitForChat();
      await ask(MESSAGE);
      await waitForReplyDone();
      await shot("devices-a-chat");
      // The manifest (with ai_configs) and the conversation go to this
      // device's Vault now.
      const backup = await invoke<Record<string, unknown>>("vault_backup_now");
      console.log(`e2e: backup ${JSON.stringify(backup).slice(0, 300)}`);
    } else if (PHASE === "b") {
      await waitForChat();
      const st = await escrow();
      console.log(`e2e: escrow ${JSON.stringify(st)}`);
      // The round the app runs 40 s after start may already have merged
      // (it did, live): the totals say whether any round changed something.
      type Sync = { updated: number; added: number; total_updated: number; total_added: number; devices: { name: string | null; needs_update: boolean }[] };
      let sync: Sync | null = null;
      const changed = (s: Sync | null) => !!s && s.devices.length > 0 && s.total_updated + s.total_added > 0;
      for (let i = 0; i < 10 && !changed(sync); i++) {
        if (i) await new Promise((r) => setTimeout(r, 6000));
        try { sync = await invoke<Sync>("config_sync_now"); } catch (e) { console.log(`e2e: config_sync_now said ${String(e).slice(0, 200)}`); }
        console.log(`e2e: config sync ${JSON.stringify(sync)}`);
      }
      expect(sync?.devices.length).toBeGreaterThan(0);
      expect((sync?.total_updated ?? 0) + (sync?.total_added ?? 0)).toBeGreaterThan(0);
      expect(sync?.devices.some((d) => d.needs_update)).toBe(false);
      await yourAis();
      await browser.waitUntil(async () => (await bodyText()).includes(NAME), { timeout: 60_000, timeoutMsg: `B never showed ${NAME}` });
      await shot("devices-b-your-ais");
      // Every AI here has this device's own agent (the fold keeps it; a new
      // one is provisioned by the context after the sync event).
      const ais = await browser.execute(async (path: string) => {
        const t = (window as any).__TAURI__.core;
        const rid = await t.invoke("plugin:store|load", { path, autoSave: false });
        const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
        const list = Array.isArray(got) ? got[0] : got;
        return (Array.isArray(list) ? list : []).map((a: any) => ({ name: a.name, agent: (a.agentPubKey ?? "").slice(0, 10), founding: (a.foundingKey ?? "").slice(0, 10), slot: a.seedSlot }));
      }, await invoke<string>("profile_store_path", { name: "ai-data.json" }));
      console.log(`e2e: B's AIs ${JSON.stringify(ais)}`);
      const renamed = ais.find((a: any) => a.name === NAME);
      expect(renamed).toBeDefined();
      expect(renamed.agent.length).toBeGreaterThan(0);
      expect(renamed.founding).not.toBe(renamed.agent);
      // A's rule, with A off: folded from A's manifest copy into this
      // device's rows cache, listed on Your Memory, read by every turn.
      const cachedRules = await invoke<{ kind: string; text: string }[]>("list_memory_rows_cached", { scope: "person" });
      console.log(`e2e: B's cached rules ${JSON.stringify(cachedRules.filter((r) => r.kind === "rule").map((r) => r.text))}`);
      expect(cachedRules.some((r) => r.kind === "rule" && r.text === RULE)).toBe(true);
      // The AI's note too, under the same founding key on this device's AI.
      const renamedB = (await aiList()).find((a) => a.name === NAME);
      const aiCached = await invoke<{ kind: string; text: string }[]>("list_memory_rows_cached", { scope: `ai:${renamedB!.founding}` });
      console.log(`e2e: B's cached AI notes ${JSON.stringify(aiCached.map((r) => r.text))}`);
      expect(aiCached.some((r) => r.text === AI_NOTE)).toBe(true);
      await browser.url("tauri://localhost/your-memory/");
      const ruleEl = await $(`[data-testid="memory-rule"]*=${RULE.slice(0, 30)}`);
      await ruleEl.waitForDisplayed({ timeout: 60_000 });
      await shot("devices-b-rule-from-a");
      // A's conversation, read-only from the Vault's copy: device A is off,
      // so its records cannot have arrived; the drawer lists the copy and the
      // chat opens it with the quiet line; a send is refused with the reason.
      await waitForChat();
      await (await $('button[title^="Conversations"]')).click();
      const copyItem = await $('[data-testid="conversation-item"][data-copy="1"]');
      await copyItem.waitForDisplayed({ timeout: 60_000 });
      const items = await browser.execute(() => [...document.querySelectorAll('[data-testid="conversation-item"]')].map((d) => `${d.getAttribute("data-copy")}:${(d as HTMLElement).innerText.replace(/\s+/g, " ").slice(0, 80)}`));
      console.log(`e2e: drawer ${JSON.stringify(items)}`);
      expect(await copyItem.getText()).toContain("from another of your devices");
      await shot("devices-b-drawer");
      await (await copyItem.$("button")).click();
      // The state lives in the composer: the reason where the person would
      // type, typing off, Send disabled. No line across the top.
      const reason = await $('[data-testid="chat-readonly-reason"]');
      await reason.waitForDisplayed({ timeout: 60_000 });
      console.log(`e2e: composer reads ${JSON.stringify(await reason.getText())}`);
      expect(await reason.getText()).toMatch(/^From \S/);
      expect(await reason.getText()).toContain("read-only until it arrives");
      expect(await (await $('[data-testid="chat-input"]')).getAttribute("contenteditable")).toBe("false");
      expect(await $('[data-testid="copied-from-line"]').isExisting()).toBe(false);
      await browser.waitUntil(async () => (await $$('[data-testid="chat-message"]')).length >= 2, { timeout: 30_000, timeoutMsg: "the copied conversation showed no messages" });
      const texts = await browser.execute(() => [...document.querySelectorAll('[data-testid="chat-message"]')].map((m) => (m as HTMLElement).innerText.slice(0, 60)));
      console.log(`e2e: copied messages ${JSON.stringify(texts)}`);
      expect(texts.some((t: string) => t.includes("Devices test"))).toBe(true);
      await shot("devices-b-copied-conversation");
      expect(await (await $('[data-testid="chat-send"]')).isEnabled()).toBe(false);
      expect(await (await $('[data-testid="chat-send"]')).getAttribute("title")).toContain("read-only until it arrives");
      await shot("devices-b-read-only-composer");

      // Settings names the device it synced with - in the signed-in panel.
      // A test identity on the staging API cannot complete the proxy
      // sign-in (only the Vault link is real here), so the panel renders
      // signed out on this harness; the line is checked when it can be.
      const sess = await invoke<{ signed_in: boolean }>("flowsta_session");
      await browser.url("tauri://localhost/settings/");
      if (sess.signed_in) {
        const line = await $('[data-testid="flowsta-devices-line"]');
        await line.waitForDisplayed({ timeout: 60_000 });
        console.log(`e2e: settings line ${JSON.stringify(await line.getText())}`);
        expect(await line.getText()).toContain("Synced with");
      } else {
        console.log("e2e: no proxy session on a test identity - the Settings line needs the signed-in panel (checked by hand / the signed-in flow)");
      }
      await shot("devices-b-settings");
    } else if (PHASE === "c") {
      // Both devices are up on the rendezvous: A (unattended, default ports)
      // holds the conversation's records; this is B on shifted ports. The
      // Vault's copy is read-only until the records arrive by gossip - here
      // they must, and the drawer item stops being a copy.
      const started = Date.now();
      await waitForChat();
      await (await $('button[title^="Conversations"]')).click();
      const drawer = () => browser.execute(() => [...document.querySelectorAll('[data-testid="conversation-item"]')].map((d) => ({ copy: d.getAttribute("data-copy"), text: (d as HTMLElement).innerText.slice(0, 80) })));
      await browser.waitUntil(async () => (await drawer()).some((d: { text: string }) => d.text.includes("Devices test")), { timeout: 60_000, timeoutMsg: "the drawer never listed A's conversation at all" });
      console.log(`e2e: drawer at start ${JSON.stringify(await drawer())}`);
      // Gossip between two conductors on one seed found peers in ~30 s in the
      // Rust live leg; allow the lane a few minutes here.
      await browser.waitUntil(async () => {
        const items = await drawer();
        return items.some((d: { copy: string | null; text: string }) => d.text.includes("Devices test") && d.copy !== "1");
      }, { timeout: 360_000, interval: 5_000, timeoutMsg: "A's conversation never arrived in B's own records (still only the Vault copy after 6 min)" });
      console.log(`e2e: arrived after ${Math.round((Date.now() - started) / 1000)} s: ${JSON.stringify(await drawer())}`);
      await shot("devices-c-arrived");
      // The arrived item: a conversation-item with the message's text that is
      // no longer a copy (clicked from the page - the driver's selector
      // language has no :not() with a text match).
      await browser.execute(() => {
        const items = [...document.querySelectorAll('[data-testid="conversation-item"]')] as HTMLElement[];
        const hit = items.find((d) => d.innerText.includes("Devices test") && d.getAttribute("data-copy") !== "1");
        (hit?.querySelector("button") as HTMLElement | null)?.click();
      });
      await browser.waitUntil(async () => (await $$('[data-testid="chat-message"]')).length >= 2, { timeout: 60_000, timeoutMsg: "the arrived conversation showed no messages" });
      const texts = await browser.execute(() => [...document.querySelectorAll('[data-testid="chat-message"]')].map((m) => (m as HTMLElement).innerText.slice(0, 60)));
      console.log(`e2e: arrived messages ${JSON.stringify(texts)}`);
      expect(texts.some((t: string) => t.includes("Devices test"))).toBe(true);
      // The composer is open: no read-only reason, typing on, Send enabled.
      expect(await $('[data-testid="chat-readonly-reason"]').isExisting()).toBe(false);
      expect(await (await $('[data-testid="chat-input"]')).getAttribute("contenteditable")).not.toBe("false");
      await shot("devices-c-composer-open");
      // And B continues the conversation in its own records.
      await ask(MESSAGE_C);
      await waitForReplyDone();
      const after = await browser.execute(() => [...document.querySelectorAll('[data-testid="chat-message"]')].map((m) => (m as HTMLElement).innerText.slice(0, 60)));
      expect(after.some((t: string) => t.includes("Devices test two"))).toBe(true);
      await shot("devices-c-continued");
    } else {
      throw new Error("set DEVICES_PHASE to a, b or c");
    }
  });
});
