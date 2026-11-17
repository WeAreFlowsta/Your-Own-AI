// The Projects helper follows the app by itself: once the person has a
// model, the app downloads the release it is pinned to, checks the download
// against the pinned fingerprint, unpacks it into this profile and says
// which version is installed. A removal is remembered, and asking for it
// brings it back; another version than the pinned one is replaced. Real downloads (about 70 MB each), so it runs only when
// asked for: YOAI_E2E_LONG, and YOAI_BLOCK_MODEL_DOWNLOADS=0 (test launches
// switch downloads off, and the background check then fetches nothing).
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { waitForChat, invoke, invokeLong } from "./_helpers";

type Status = {
  installed: boolean;
  path: string | null;
  installed_version: string | null;
  pinned_version: string;
  downloading: boolean;
  error: string | null;
  removed: boolean;
};
const status = () => invoke<Status>("build_install_status");

describe("the Projects helper follows the app", () => {
  it("arrives by itself, stays removed when removed, and comes back when asked for", async function () {
    if (!process.env.YOAI_E2E_LONG || process.env.YOAI_BLOCK_MODEL_DOWNLOADS !== "0") this.skip();
    this.timeout(1_800_000);
    await waitForChat();
    const before = await status();
    console.log(`e2e: helper before ${JSON.stringify(before)}`);
    expect(before.removed).toBe(false);

    // Nobody presses anything: the app's own check (a little after start)
    // fetches it. The tray carries the download meanwhile.
    let sawTrayRow = false;
    await browser.waitUntil(
      async () => {
        if (!sawTrayRow) {
          sawTrayRow = await browser.execute(() =>
            Array.from(document.querySelectorAll("[data-testid='activity-tray'] *")).some((n) => n.textContent === "Projects helper"),
          );
        }
        return (await status()).installed;
      },
      { timeout: 840_000, interval: 2_000, timeoutMsg: "the helper did not arrive by itself" },
    );
    const arrived = await status();
    console.log(`e2e: helper arrived by itself ${JSON.stringify(arrived)}`);
    expect(sawTrayRow).toBe(true);
    expect(arrived.installed_version).toBe(arrived.pinned_version);
    expect(arrived.error).toBeNull();
    // In this test profile, never in the person's own app folder.
    expect(arrived.path).toContain("/e2e/profile/");
    expect(await invoke<boolean>("path_is_file", { path: arrived.path })).toBe(true);
    // The page learned where it is, with no install button pressed.
    await browser.waitUntil(
      async () => (await browser.execute(() => localStorage.getItem("build-binary-path"))) === arrived.path,
      { timeout: 20_000, timeoutMsg: "the page did not learn the helper's path" },
    );
    // Another check finds nothing to do.
    await browser.waitUntil(async () => !(await status()).downloading, { timeout: 30_000 });
    expect(await invoke<string>("ensure_build_agent")).toBe("current");

    // An app update that pins another version: the installed helper says
    // it is an earlier one, and the check replaces it with no button.
    writeFileSync(join(dirname(arrived.path!), "VERSION"), "0.4.2");
    const older = await status();
    expect(older.installed_version).toBe("0.4.2");
    expect(older.update_available).toBe(true);
    expect(await invokeLong<string>("ensure_build_agent", {}, 840_000)).toBe("updated");
    const replaced = await status();
    console.log(`e2e: helper replaced by itself ${JSON.stringify(replaced)}`);
    expect(replaced.installed_version).toBe(replaced.pinned_version);
    expect(replaced.update_available).toBe(false);

    // Removed by the person: remembered, and the check leaves it alone.
    await invoke("uninstall_build_agent");
    const removed = await status();
    expect(removed.installed).toBe(false);
    expect(removed.removed).toBe(true);
    expect(await invoke<string>("ensure_build_agent")).toBe("removed");
    expect((await status()).installed).toBe(false);

    // Asked for (the card's or the door's Download): it comes back, and the
    // removal no longer stands.
    const path = await invokeLong<string>("download_build_agent", {}, 840_000);
    const back = await status();
    console.log(`e2e: helper after asking ${JSON.stringify(back)}`);
    expect(back.installed).toBe(true);
    expect(back.removed).toBe(false);
    expect(back.installed_version).toBe(back.pinned_version);
    expect(path).toBe(arrived.path);

    // Not left installed for the specs that follow (each run starts from a
    // wiped profile, so the removal note goes with it).
    await invoke("uninstall_build_agent");
    expect((await status()).installed).toBe(false);
  });
});
