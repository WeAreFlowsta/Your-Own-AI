// The island merge, live (build-docs MULTI_DEVICE.md §5.1): a device that
// used Your Own AI before it was signed in joins the identity's network.
// Three launches, one phase each (ISLAND_PHASE), against a TEST Vault on
// a pinned port (YOAI_VAULT_PORT) - never the installed one:
//   a   profile A, fresh: links, escrows its material for the identity
//   b0  profile B, fresh and NOT linked: chats on its own material
//   b1  profile B, now linked (fixture): the startup sync finds the
//       identity's material elsewhere, exports, swaps, and the app exits
//   b2  profile B again: the startup restore replays the export; the
//       drawer shows the conversation under the identity's material
// Run: e2e/island-live.sh
import { shot, waitForChat, ask, waitForReplyDone, invoke } from "../models/_helpers";

const PHASE = process.env.ISLAND_PHASE ?? "";
const MESSAGE = "Island test: name one colour and nothing else.";

// The link itself is a fixture (island-live.sh prelink): the test Vault's
// API does not know this app, so the profile starts linked.
const escrow = () => invoke<{ state: string; local_conversations?: number | null }>("vault_escrow_sync");

describe(`island merge, phase ${PHASE}`, () => {
  it(PHASE === "a" ? "A: escrows the identity's material" : PHASE === "b0" ? "B: chats before it is linked" : PHASE === "b1" ? "B: linked, the merge runs by itself" : "B: comes back with its conversation under the identity", async () => {
    if (PHASE === "a") {
      await waitForChat();
      const st = await escrow();
      console.log(`e2e: escrow ${JSON.stringify(st)}`);
      expect(st.state).toBe("synced");
    } else if (PHASE === "b0") {
      // Used before sign-in: a conversation on this device's own material.
      await waitForChat();
      const before = await escrow();
      console.log(`e2e: escrow before linking ${JSON.stringify(before)}`);
      expect(before.state).toBe("unlinked");
      await ask(MESSAGE);
      await waitForReplyDone();
      await shot("island-b0-chat");
    } else if (PHASE === "b1") {
      // Linked now (the fixture): the startup sync finds the identity's
      // material elsewhere, exports this device's conversation, swaps the
      // material and exits (a debug build exits instead of restarting).
      await waitForChat();
      let gone = false;
      for (let i = 0; i < 60 && !gone; i++) {
        await new Promise((r) => setTimeout(r, 3000));
        try { await browser.execute(() => 1); } catch { gone = true; }
      }
      console.log(`e2e: app ${gone ? "exited for the swap" : "still running after 180 s"}`);
      expect(gone).toBe(true);
    } else if (PHASE === "b2") {
      // The startup restore runs this itself; calling it here too makes its
      // answer (or its error) visible in the run log.
      await waitForChat();
      for (let i = 0; i < 20; i++) {
        try {
          const r = await invoke<unknown>("vault_restore_conversations");
          console.log(`e2e: restore answered ${JSON.stringify(r).slice(0, 300)}`);
          break;
        } catch (e) {
          const msg = String(e);
          console.log(`e2e: restore said ${msg.slice(0, 200)}`);
          if (!msg.includes("still starting")) break;
          await new Promise((r) => setTimeout(r, 5000));
        }
      }
      await browser.waitUntil(async () => (await invoke<boolean>("vault_restore_pending")) === false, {
        timeout: 300_000,
        timeoutMsg: "the island merge never finished",
      });
      await waitForChat();
      const st = await escrow();
      console.log(`e2e: escrow after the merge ${JSON.stringify(st)}`);
      expect(st.state).toBe("synced");
      await (await $('button[title^="Conversations - pick up"]')).click();
      try {
        await browser.waitUntil(async () => (await $$('button[title="Rename"]')).length >= 1, {
          timeout: 90_000,
          timeoutMsg: "the drawer shows no conversation after the merge",
        });
      } catch (e) {
        await shot("island-b2-drawer-empty");
        const text = await browser.execute(() => (document.body.innerText || "").replace(/\s+/g, " ").slice(0, 600));
        console.log(`e2e: page text: ${text}`);
        throw e;
      }
      const titles = await browser.execute(() =>
        [...document.querySelectorAll('button[title="Rename"]')].map((b) => (b.parentElement?.querySelector("span")?.textContent || "").trim()),
      );
      console.log(`e2e: drawer after the merge ${JSON.stringify(titles)}`);
      await shot("island-b2-drawer");
      expect(titles.length).toBeGreaterThanOrEqual(1);
    } else {
      throw new Error("set ISLAND_PHASE to a, b0, b1 or b2");
    }
  });
});
