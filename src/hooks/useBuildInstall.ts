import { useSignal, useVisibleTask$, $, type Signal, type QRL } from "@builder.io/qwik";

/**
 * Your Own AI Build on this computer: is it installed, is it downloading,
 * how far along, what went wrong - and the one action that installs it.
 * Shared, so every place that offers the install (the Build card, a tool's
 * Set up checklist) says the same thing from the same facts. The truth lives
 * in Rust (`build_install_status`, a pinned release, a single-flight
 * download that keeps going across pages); this only listens to it.
 */
export interface BuildInstall {
  installed: Signal<boolean>;
  /** Installed, but older than the version this app ships. */
  updateAvailable: Signal<boolean>;
  installedVersion: Signal<string>;
  pinnedVersion: Signal<string>;
  downloading: Signal<boolean>;
  percent: Signal<number>;
  error: Signal<string | null>;
  install$: QRL<() => void>;
}

export function useBuildInstall(): BuildInstall {
  const installed = useSignal(false);
  const updateAvailable = useSignal(false);
  const installedVersion = useSignal("");
  const pinnedVersion = useSignal("");
  const downloading = useSignal(false);
  const percent = useSignal(0);
  const error = useSignal<string | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    let gone = false;
    const uns: (() => void)[] = [];
    cleanup(() => {
      gone = true;
      uns.forEach((fn) => fn());
    });
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const st = (await invoke("build_install_status")) as {
        installed: boolean;
        installed_version: string | null;
        pinned_version: string;
        update_available: boolean;
        downloading: boolean;
        error: string | null;
      };
      installed.value = st.installed;
      updateAvailable.value = st.update_available;
      installedVersion.value = st.installed_version ?? "";
      pinnedVersion.value = st.pinned_version;
      downloading.value = st.downloading;
      error.value = st.error;
    } catch {
      /* stays in its defaults */
    }
    const { listen } = await import("@tauri-apps/api/event");
    const keep = (fn: () => void) => (gone ? fn() : uns.push(fn));
    keep(
      await listen<any>("model-download-progress", (e) => {
        const f = e.payload?.filename;
        if (typeof f === "string" && f.startsWith("your-own-ai-build-")) {
          downloading.value = true;
          percent.value = Math.round(e.payload?.percent ?? 0);
        }
      }),
    );
    keep(
      await listen<any>("build-install-done", (e) => {
        downloading.value = false;
        installed.value = true;
        updateAvailable.value = false;
        installedVersion.value = pinnedVersion.value;
        error.value = null;
        const path = e.payload?.path;
        if (typeof path === "string" && path) {
          try {
            localStorage.setItem("build-binary-path", path);
          } catch { /* resolver falls back */ }
        }
      }),
    );
    keep(
      await listen<any>("build-install-failed", (e) => {
        downloading.value = false;
        error.value = String(e.payload?.error ?? "download failed");
      }),
    );
    keep(
      await listen<any>("build-uninstalled", () => {
        installed.value = false;
      }),
    );
  });

  const install$ = $(async () => {
    error.value = null;
    downloading.value = true;
    percent.value = 0;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      invoke("download_build_agent").catch((e) => {
        // Guard rejections (e.g. "Close the open project first, then
        // update.") return straight from the command without firing the
        // failure event - surface them here or the button spins forever.
        downloading.value = false;
        error.value = String(e);
      });
    } catch {
      downloading.value = false;
    }
  });

  return { installed, updateAvailable, installedVersion, pinnedVersion, downloading, percent, error, install$ };
}
