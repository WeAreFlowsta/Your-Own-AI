// Not a take: signs a kept shoot profile in to Flowsta from Settings (when the welcome's step 3 did not finish).
// The Vault asks "Connect Your Own AI?"; a person clicks Allow there. Prints the session's name and agent key.
//   YOAI_E2E_KEEP=1 YOAI_E2E_HOME=<shoot folder> YOAI_CAPTURE_DRY=1 node e2e/run.mjs capture-fresh --spec e2e/capture/shoot-connect.capture.ts
import { appUrl } from "../wdio.conf";

type Session = { agent_pub_key: string | null; display_name?: string | null; username?: string | null };
const session = () => browser.execute(async () => (window as any).__TAURI__.core.invoke("flowsta_session")) as Promise<Session>;

describe("capture", () => {
  it("shoot connect", async () => {
    await browser.url(appUrl("/settings/"));
    let s = await session().catch(() => ({ agent_pub_key: null }) as Session);
    if (!s.agent_pub_key) {
      const button = await $('button[aria-label="Sign in with Flowsta"]');
      await button.waitForExist({ timeout: 60_000 });
      await button.scrollIntoView({ block: "center" });
      await button.waitForEnabled({ timeout: 60_000 });
      await button.click();
      console.log("shoot-connect: ALLOW in the Vault now (Connect Your Own AI?)");
      await browser.waitUntil(async () => !!(s = await session().catch(() => s)).agent_pub_key, {
        timeout: 1_200_000,
        interval: 3_000,
        timeoutMsg: "never signed in: was Allow clicked in the Vault?",
      });
    }
    console.log(`shoot-connect: signed in as ${s.display_name ?? "?"} (@${s.username ?? "?"}), agent key ${s.agent_pub_key}`);
  });
});
