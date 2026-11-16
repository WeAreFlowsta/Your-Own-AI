// The Projects helper's real install: the app downloads the release it is
// pinned to, checks the download against the pinned fingerprint, unpacks it
// into this profile and says which version is installed. A real download
// (about 70 MB), so it runs only when asked for: YOAI_E2E_LONG, and
// YOAI_BLOCK_MODEL_DOWNLOADS=0 (test launches switch downloads off).
import { waitForChat, invoke, invokeLong } from "./_helpers";

type Status = { installed: boolean; path: string | null; installed_version: string | null; pinned_version: string; error: string | null };

describe("the Projects helper's install", () => {
  it("downloads the pinned release, checks it and installs it in this profile", async function () {
    if (!process.env.YOAI_E2E_LONG || process.env.YOAI_BLOCK_MODEL_DOWNLOADS !== "0") this.skip();
    this.timeout(900_000);
    await waitForChat();
    const before = await invoke<Status>("build_install_status");
    console.log(`e2e: helper before ${JSON.stringify(before)}`);
    expect(before.installed).toBe(false);
    const path = await invokeLong<string>("download_build_agent", {}, 840_000);
    const after = await invoke<Status>("build_install_status");
    console.log(`e2e: helper after ${JSON.stringify(after)}`);
    expect(after.installed).toBe(true);
    expect(after.installed_version).toBe(after.pinned_version);
    expect(after.error).toBeNull();
    // In this test profile, never in the person's own app folder.
    expect(path).toContain("/e2e/profile/");
    expect(await invoke<boolean>("path_is_file", { path })).toBe(true);
    await invoke("uninstall_build_agent");
    expect((await invoke<Status>("build_install_status")).installed).toBe(false);
  });
});
