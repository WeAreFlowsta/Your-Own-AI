// One identity on two devices, live (build-docs MULTI_DEVICE.md §5.4):
// device A renames an AI and backs up; its Vault copies the manifest to
// the identity's second Vault; device B signs in fresh there and shows the
// renamed AI with its own agent. Two launches, one phase each
// (DEVICES_PHASE), each against its own TEST Vault (YOAI_VAULT_PORT) -
// never the installed one. Run: e2e/devices-live.sh
import { shot, waitForChat, invoke } from "../models/_helpers";

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
      // The manifest (with ai_configs) goes to this device's Vault now.
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
