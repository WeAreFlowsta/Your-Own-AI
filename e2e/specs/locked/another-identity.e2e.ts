// The Vault on an identity this install was never connected under
// (e2e/another-identity-live.sh): says which profile the app opened and what
// the page reads, in phase a (the Vault holds the linked identity) and in
// phase b (it holds another). The script compares the two.
import { shot, invoke } from "../models/_helpers";

const PHASE = process.env.ANOTHER_PHASE ?? "a";

describe(`the Vault's identity and the profile opened, phase ${PHASE}`, () => {
  it("opens a profile and says who is signed in", async function () {
    this.timeout(900_000);
    // Either the chat or a first-run page: whatever this profile shows.
    await browser.waitUntil(async () => ((await browser.execute(() => document.body.innerText)) ?? "").trim().length > 40, { timeout: 600_000, interval: 2_000, timeoutMsg: "the window never showed a page" });
    await browser.pause(25_000);
    const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
    const profile = path.split(/[\\/]/).slice(-2, -1)[0];
    console.log(`e2e: profile ${profile}`);
    const where = await browser.execute(() => location.pathname);
    const text = ((await browser.execute(() => document.body.innerText)) ?? "").replace(/\s+/g, " ").trim();
    console.log(`e2e: page ${where} reads: ${text.slice(0, 360)}`);
    const auth = await invoke<any>("flowsta_vault_status").catch((e) => `no status (${String(e).slice(0, 80)})`);
    console.log(`e2e: flowsta status ${JSON.stringify(auth).slice(0, 300)}`);
    await shot(`another-identity-${PHASE}`);
    expect(profile).toBeTruthy();
  });
});
