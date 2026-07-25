/**
 * Header workspace + conversations wiring for every page EXCEPT the chat
 * route (which owns the live agent session and richer status). The bridge
 * is the single source of truth for the open workspace, so any page can
 * render the slot; actions that need the chat machinery (opening a folder,
 * browsing, the conversations drawer) hand off to /chat via sessionStorage
 * flags the chat route consumes on mount.
 */
import { $, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { useNavigate } from "@builder.io/qwik-city";
import { readRecentFolders, resolveBinaryPath } from "./useAgentSession";

export function useHeaderWorkspace() {
  const nav = useNavigate();
  const buildInstalled = useSignal(false);
  const recentFolders = useSignal<string[]>([]);
  const folderPath = useSignal<string | null>(null);
  const folderStatus = useSignal<"ready" | "stopped" | undefined>(undefined);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    recentFolders.value = readRecentFolders();
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      buildInstalled.value = await invoke<boolean>("path_is_file", {
        path: resolveBinaryPath(),
      });
      const status = await invoke<{ running: boolean; folder: string | null }>(
        "build_agent_status",
      );
      folderPath.value = status.folder;
      folderStatus.value = status.folder
        ? status.running
          ? "ready"
          : "stopped"
        : undefined;
    } catch {
      /* header extras are best-effort */
    }
    // A background install finishing flips the gate on every page.
    const { listen } = await import("@tauri-apps/api/event");
    const un = await listen("build-install-done", () => {
      buildInstalled.value = true;
    });
    cleanup(un);
  });

  const openConversations$ = $(async () => {
    try {
      sessionStorage.setItem("open-conversations", "1");
    } catch {
      /* flag is best-effort */
    }
    await nav("/chat/");
  });

  const openFolder$ = $(async (path: string) => {
    try {
      sessionStorage.setItem("pending-open-folder", path);
    } catch {
      /* flag is best-effort */
    }
    await nav("/chat/");
  });

  const browseFolder$ = $(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({ directory: true, title: "Choose the project's folder" });
      if (typeof selected === "string" && selected) await openFolder$(selected);
    } catch (err) {
      console.error("[Header] Folder picker error:", err);
    }
  });

  const closeFolder$ = $(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("stop_build_agent");
      folderPath.value = null;
      folderStatus.value = undefined;
    } catch {
      /* already stopped is fine */
    }
  });

  return {
    buildInstalled,
    recentFolders,
    folderPath,
    folderStatus,
    openConversations$,
    openFolder$,
    browseFolder$,
    closeFolder$,
  };
}
