/**
 * Header workspace + conversations wiring for every page EXCEPT the chat
 * route (which owns the live agent session and richer status). The bridge
 * is the single source of truth for the open workspace, so any page can
 * render the slot; actions that need the chat machinery (opening a folder,
 * browsing, the conversations drawer) hand off to /chat via sessionStorage
 * flags the chat route consumes on mount.
 */
import { $, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { permissionModeForFolder } from "../utils/agentPermissions";
import { useNavigate } from "@builder.io/qwik-city";
import { readRecentFolders, resolveBinaryPath } from "./useAgentSession";

export function useHeaderWorkspace() {
  const nav = useNavigate();
  const buildInstalled = useSignal(false);
  const recentFolders = useSignal<string[]>([]);
  const folderPath = useSignal<string | null>(null);
  const folderStatus = useSignal<"starting" | "ready" | "stopped" | undefined>(undefined);
  // The open project's permission mode (per-folder choice, else the Settings
  // default) - display only here; the chat route owns switching it.
  const permissionMode = useSignal<"ask" | "auto" | "all">("ask");

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    recentFolders.value = readRecentFolders();
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      buildInstalled.value = await invoke<boolean>("path_is_file", {
        path: resolveBinaryPath(),
      });
      const status = await invoke<{
        running: boolean;
        sessionId: string | null;
        folder: string | null;
      }>("build_agent_status");
      folderPath.value = status.folder;
    permissionMode.value = status.folder ? permissionModeForFolder(status.folder) : "ask";
      // Running without a session id = still starting (the orange state
      // must survive navigation - the session lives in Rust, not the page).
      folderStatus.value = status.folder
        ? status.running
          ? status.sessionId
            ? "ready"
            : "starting"
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
    // The session's lifecycle continues while this page is open - keep the
    // slot honest through startup, readiness, and death.
    const unReady = await listen("agent-ready", () => {
      if (folderPath.value) folderStatus.value = "ready";
    });
    const unExit = await listen("agent-exit", () => {
      if (folderPath.value) folderStatus.value = "stopped";
    });
    cleanup(() => {
      un();
      unReady();
      unExit();
    });
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
    permissionMode,
    openConversations$,
    openFolder$,
    browseFolder$,
    closeFolder$,
  };
}
