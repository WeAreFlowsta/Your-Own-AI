// One identity on two devices, live (build-docs MULTI_DEVICE.md §5.4):
// device A renames an AI and backs up; its Vault copies the manifest to
// the identity's second Vault; device B signs in fresh there and shows the
// renamed AI with its own agent. Two launches, one phase each
// (DEVICES_PHASE), each against its own TEST Vault (YOAI_VAULT_PORT) -
// never the installed one. Run: e2e/devices-live.sh
import { shot, waitForChat, invoke, ask, waitForReplyDone, typeInto } from "../models/_helpers";

const MESSAGE = "Devices test: name one colour and nothing else.";

const PHASE = process.env.DEVICES_PHASE ?? "";
const NAME = process.env.DEVICES_NAME ?? "Synced AI";

const escrow = () => invoke<{ state: string }>("vault_escrow_sync");
const bodyText = () => browser.execute(() => document.body.innerText);

async function yourAis() {
  await browser.url("tauri://localhost/your-ais/");
  await browser.waitUntil(async () => (await $$('button[title^="Edit AI"]')).length > 0, { timeout: 60_000, timeoutMsg: "the Your AIs page never listed an AI" });
}

describe(`devices, phase ${PHASE}`, () => {
  it(PHASE === "a" ? "A: renames an AI through the form and backs up" : "B: signs in fresh and shows A's edit with its own agent", async () => {
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
      const line = await $('[data-testid="copied-from-line"]');
      await line.waitForDisplayed({ timeout: 60_000 });
      console.log(`e2e: copied-from line ${JSON.stringify(await line.getText())}`);
      expect(await line.getText()).toContain("LinuxLappy");
      await browser.waitUntil(async () => (await $$('[data-testid="chat-message"]')).length >= 2, { timeout: 30_000, timeoutMsg: "the copied conversation showed no messages" });
      const texts = await browser.execute(() => [...document.querySelectorAll('[data-testid="chat-message"]')].map((m) => (m as HTMLElement).innerText.slice(0, 60)));
      console.log(`e2e: copied messages ${JSON.stringify(texts)}`);
      expect(texts.some((t: string) => t.includes("Devices test"))).toBe(true);
      await shot("devices-b-copied-conversation");
      await typeInto("chat-input", "Can I add to this?");
      await (await $('[data-testid="chat-send"]')).click();
      await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("You can read it here"), { timeout: 15_000, timeoutMsg: "the send was not refused with the reason" });
      expect((await $$('[data-testid="chat-message"]')).length).toBeLessThanOrEqual(texts.length);
      await shot("devices-b-send-refused");

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
    } else {
      throw new Error("set DEVICES_PHASE to a or b");
    }
  });
});
