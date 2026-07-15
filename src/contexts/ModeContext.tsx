/**
 * ModeContext (Qwik) - Manages local vs cloud mode for Desktop app
 *
 * This context tracks whether the user is using:
 * - Local Mode: Free, private, offline with llama.cpp
 * - Cloud Mode: Paid, fast, with cloud inference
 *
 * Mode is persisted using Tauri Store so it survives app restarts.
 */

import {
  createContextId,
  useContext,
  useContextProvider,
  useStore,
  useVisibleTask$,
  $,
  component$,
  Slot,
  type NoSerialize,
  noSerialize,
} from "@builder.io/qwik";
import { Store } from "@tauri-apps/plugin-store";

export type AppMode = "local" | "cloud";

export interface ModeContextType {
  mode: AppMode;
  isAuthenticated: boolean;
  isLoading: boolean;
  store: NoSerialize<Store> | undefined;
}

export const ModeContext = createContextId<ModeContextType>("app.mode");

const STORE_PATH = "settings.json";
const STORE_MODE_FIELD = "app-mode";
const STORE_LOGIN_FIELD = "is-authenticated";

export const ModeProvider = component$(() => {
  const state = useStore<ModeContextType>({
    mode: "local",
    isAuthenticated: false,
    isLoading: true,
    store: undefined,
  });

  // Initialize store and load saved mode (browser-only)
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const storeInstance = await Store.load(STORE_PATH);
      state.store = noSerialize(storeInstance);

      // Load saved mode
      const savedMode = await storeInstance.get<AppMode>(STORE_MODE_FIELD);
      if (savedMode === "local" || savedMode === "cloud") {
        state.mode = savedMode;
        console.log(`[ModeContext] Loaded saved mode: ${savedMode}`);
      } else {
        state.mode = "local";
        console.log("[ModeContext] No saved mode, defaulting to local");
      }

      // Load auth status
      const savedAuth = await storeInstance.get<boolean>(STORE_LOGIN_FIELD);
      if (savedAuth !== null && savedAuth !== undefined) {
        state.isAuthenticated = savedAuth;
        console.log(`[ModeContext] Loaded auth status: ${savedAuth}`);
      }
    } catch (error) {
      console.error("[ModeContext] Error initializing store:", error);
      state.mode = "local";
    } finally {
      state.isLoading = false;
    }
  });

  useContextProvider(ModeContext, state);

  return <Slot />;
});

/**
 * Hook to access mode context
 */
export function useMode() {
  return useContext(ModeContext);
}

/**
 * Hook to check if app is in local mode
 */
export function useIsLocalMode(): boolean {
  const ctx = useMode();
  return ctx.mode === "local";
}

/**
 * Hook to check if app is in cloud mode
 */
export function useIsCloudMode(): boolean {
  const ctx = useMode();
  return ctx.mode === "cloud";
}

/**
 * Returns QRL actions for mode switching.
 * Must be called from within a component$.
 */
export function useModeActions() {
  const state = useMode();

  const setMode = $((newMode: AppMode) => {
    console.log(`[ModeContext] Switching mode: ${state.mode} → ${newMode}`);
    state.mode = newMode;

    // Persist to store
    const s = state.store;
    if (s) {
      s.set(STORE_MODE_FIELD, newMode).then(() => s.save());
    }
  });

  const switchToCloud = $(() => {
    if (!state.isAuthenticated) {
      console.log("[ModeContext] Not authenticated. Opening login page...");
      const websiteUrl =
        (import.meta as any).env?.VITE_WEBSITE_URL || "https://yourownai.net";
      window.open(`${websiteUrl}/pricing`, "_blank");
    } else {
      console.log("[ModeContext] Switching to cloud mode");
      state.mode = "cloud";
      const s = state.store;
      if (s) {
        s.set(STORE_MODE_FIELD, "cloud" as AppMode).then(() => s.save());
      }
    }
  });

  const switchToLocal = $(() => {
    console.log("[ModeContext] Switching to local mode");
    state.mode = "local";
    const s = state.store;
    if (s) {
      s.set(STORE_MODE_FIELD, "local" as AppMode).then(() => s.save());
    }
  });

  const setIsAuthenticated = $((auth: boolean) => {
    state.isAuthenticated = auth;
    const s = state.store;
    if (s) {
      s.set(STORE_LOGIN_FIELD, auth).then(() => s.save());
    }
  });

  return { setMode, switchToCloud, switchToLocal, setIsAuthenticated };
}
