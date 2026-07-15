/**
 * AiDataContext (Qwik) - Manages AI personalities and custom AIs
 *
 * State is held in a reactive store via createContextId.
 * Actions are provided via useAiDataActions() hook.
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
} from "@builder.io/qwik";
import type {
  Archetype,
  UserDefinedAI,
  CreateUserAiData,
  UpdateUserAiData,
  ResponseLengthOption,
} from "../types";
import { bundledArchetypes } from "../data/bundled-archetypes";
import { responseLengthOptions as bundledResponseLengths } from "../data/response-lengths";
import { invoke } from "@tauri-apps/api/core";
import {
  getLocalCustomAis,
  saveLocalCustomAi,
  updateLocalCustomAi,
  deleteLocalCustomAi,
  updateLocalCustomAiStatus,
  seedDefaultAisIfNeeded,
  republishStoredPersonas,
} from "../utils/localAiStorage";

export interface AiDataState {
  archetypeTemplates: Archetype[];
  userDefinedAis: UserDefinedAI[];
  thumbnailObjectUrls: Record<string, string>;
  archetypeDefaultThumbnailsMap: Record<string, string>;
  responseLengthOptions: ResponseLengthOption[];
  isLoadingArchetypes: boolean;
  isLoadingUserAis: boolean;
  isInitialized: boolean;
  archetypesError: string | null;
  userAisError: string | null;
}

export const AiDataContext = createContextId<AiDataState>("app.aidata");

export const AiDataProvider = component$(() => {
  const state = useStore<AiDataState>(
    {
      archetypeTemplates: [],
      userDefinedAis: [],
      thumbnailObjectUrls: {},
      archetypeDefaultThumbnailsMap: {},
      responseLengthOptions: bundledResponseLengths,
      isLoadingArchetypes: false,
      isLoadingUserAis: false,
      isInitialized: false,
      archetypesError: null,
      userAisError: null,
    },
    { deep: true }
  );

  // Load archetypes, user AIs, and preferences on mount
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    // Load archetypes (always bundled for now)
    state.isLoadingArchetypes = true;
    try {
      state.archetypeTemplates = bundledArchetypes;
      console.log(
        `[AiDataContext] Loaded ${bundledArchetypes.length} bundled archetypes`
      );

      const thumbnailsMap: Record<string, string> = {};
      bundledArchetypes.forEach((archetype) => {
        thumbnailsMap[archetype.id] =
          archetype.thumbnailPath || "/generic-ai-placeholder.svg";
      });
      state.archetypeDefaultThumbnailsMap = thumbnailsMap;
    } catch (error) {
      console.error("[AiDataContext] Error loading archetypes:", error);
      state.archetypesError =
        error instanceof Error ? error.message : "Failed to load archetypes";
      state.archetypeTemplates = bundledArchetypes;
    } finally {
      state.isLoadingArchetypes = false;
    }

    // Helper to update loading overlay text
    const setLoadingText = (text: string) => {
      const el = document.getElementById("app-loading-text");
      if (el) el.textContent = text;
    };

    // Seed default AIs on first launch (migrates localStorage prefs if any)
    await seedDefaultAisIfNeeded();

    // Keep each AI's stored prompt = current mission-core + live persona, so the
    // inference API (which reads the stored prompt) matches in-app chat.
    await republishStoredPersonas();

    // Finish a device restore begun with "Restore key from Vault": that flow
    // relaunches the app holding the right key but none of the conversations,
    // leaving a restore-pending marker. Complete the user's one intent here,
    // visibly, before the AI list loads - this happens once, on a new machine,
    // and nobody should have to find a second button. The conductor may still
    // be starting, so retry; any other failure leaves the marker in place and
    // the manual button in Settings as the fallback.
    try {
      const restorePending = await invoke<boolean>("vault_restore_pending");
      if (restorePending) {
        setLoadingText("Restoring your conversations from your Vault…");
        for (let attempt = 0; attempt < 15; attempt++) {
          try {
            const stats = await invoke("vault_restore_conversations");
            console.log("[AiDataContext] Startup restore complete:", stats);
            break;
          } catch (e) {
            if (String(e).includes("still starting")) {
              await new Promise((r) => setTimeout(r, 2000));
            } else {
              console.warn("[AiDataContext] Startup restore failed (Settings button remains):", e);
              break;
            }
          }
        }
      }
    } catch (e) {
      console.warn("[AiDataContext] Restore-pending check failed:", e);
    }

    // Load all AIs (defaults + custom, now unified in one store)
    setLoadingText("Loading your AIs...");
    state.isLoadingUserAis = true;
    try {
      const localAis = await getLocalCustomAis();
      state.userDefinedAis = localAis;
      console.log(
        `[AiDataContext] Loaded ${localAis.length} AIs from local storage`
      );
    } catch (error) {
      console.error("[AiDataContext] Error loading AIs:", error);
      state.userAisError =
        error instanceof Error ? error.message : "Failed to load AIs";
      state.userDefinedAis = [];
    } finally {
      state.isLoadingUserAis = false;
    }

    // Provision Holochain agents for all AIs (assigns agent pub keys)
    if (state.userDefinedAis.length > 0) {
      try {
        const { provisionAllAgents } = await import(
          "../utils/holochainTranscripts"
        );
        const aiIds = state.userDefinedAis.map((ai) => ai.id);
        setLoadingText("Setting up AI agents...");
        console.log(`[AiDataContext] Provisioning ${aiIds.length} AI agents...`);

        // Retry provisioning — conductor may still be starting
        let agentKeys: Record<string, string> = {};
        for (let attempt = 0; attempt < 10; attempt++) {
          agentKeys = await provisionAllAgents(aiIds);
          if (Object.keys(agentKeys).length > 0) break;
          console.log(`[AiDataContext] Conductor not ready, retrying... (${attempt + 1}/10)`);
          await new Promise((r) => setTimeout(r, 2000));
        }

        // Set AI IDs to their agent pub keys (the canonical identifier)
        for (const ai of state.userDefinedAis) {
          const pubKey = agentKeys[ai.id];
          if (pubKey && ai.id !== pubKey) {
            const oldId = ai.id;
            ai.agentPubKey = pubKey;
            ai.id = pubKey;
            await updateLocalCustomAi(oldId, { id: pubKey, agentPubKey: pubKey } as any);
            // Migrate thumbnail to new ID
            try {
              const thumbData = await invoke<number[]>('get_ai_thumbnail', { aiId: oldId });
              await invoke('save_ai_thumbnail', { aiId: pubKey, thumbnailData: thumbData });
              await invoke('delete_ai_thumbnail', { aiId: oldId });
            } catch {
              // No thumbnail to migrate
            }
            console.log(`[AiDataContext] AI ${ai.name}: ${oldId} → ${pubKey.slice(0, 16)}...`);
          } else if (pubKey) {
            ai.agentPubKey = pubKey;
          }
        }
      } catch (e) {
        console.warn("[AiDataContext] Agent provisioning failed (non-fatal):", e);
      }
    }

    // After a device restore, rebuild the per-AI memory index (episodic
    // recall + authored-knowledge vectors) in the background - no-op unless
    // the restore left its marker and the embedding model is present.
    import("../utils/transcriptMemory")
      .then(({ rebuildMemoryIndexIfPending }) => rebuildMemoryIndexIfPending())
      .catch((e) => console.warn("[AiDataContext] Memory re-embed check failed:", e));

    setLoadingText("Loading thumbnails...");
    // Load thumbnails for custom AIs (before marking initialized so
    // downstream consumers see complete data on first render)
    if (state.userDefinedAis.length > 0) {
      const newUrls: Record<string, string> = {};
      for (const ai of state.userDefinedAis) {
        try {
          const thumbnailData = await invoke<number[]>("get_ai_thumbnail", {
            aiId: ai.id,
          });
          const uint8Array = new Uint8Array(thumbnailData);
          const blob = new Blob([uint8Array], { type: "image/jpeg" });
          newUrls[ai.id] = URL.createObjectURL(blob);
        } catch {
          // No custom thumbnail, will use archetype default
        }
      }
      state.thumbnailObjectUrls = newUrls;
    }

    state.isInitialized = true;

    // Dismiss the loading overlay now that all data is ready
    // (archetypes + AIs + agent provisioning + thumbnails)
    const loader = document.getElementById("app-loading");
    if (loader) {
      loader.style.opacity = "0";
      setTimeout(() => {
        loader.style.display = "none";
      }, 300);
    }
  });

  useContextProvider(AiDataContext, state);

  return <Slot />;
});

/**
 * Hook to access AI data state
 */
export function useAiData() {
  return useContext(AiDataContext);
}

/**
 * Pure utility — checks if an AI can be deactivated (must keep at least 1 active).
 */
export function canDeactivate(
  state: AiDataState,
  aiId: string
): boolean {
  const activeAis = state.userDefinedAis.filter(
    (ai) => ai.status === "active"
  );

  if (activeAis.length <= 1) {
    const ai = state.userDefinedAis.find((a) => a.id === aiId);
    return ai?.status !== "active";
  }
  return true;
}

/**
 * Hook providing action QRLs for AI data mutations.
 * Must be called from within a component$.
 */
export function useAiDataActions() {
  const state = useAiData();

  const createCustomAi = $(
    async (
      ai: Omit<UserDefinedAI, "id" | "status">
    ): Promise<UserDefinedAI> => {
      const newAi = await saveLocalCustomAi(ai);

      // Provision Holochain agent and use pub key as canonical ID
      try {
        const { provisionAgent } = await import(
          "../utils/holochainTranscripts"
        );
        const agentKey = await provisionAgent(newAi.id);
        if (agentKey) {
          const oldId = newAi.id;
          newAi.agentPubKey = agentKey;
          newAi.id = agentKey;
          await updateLocalCustomAi(oldId, { id: agentKey, agentPubKey: agentKey } as any);
          // Migrate thumbnail if one was saved
          try {
            const thumbData = await invoke<number[]>('get_ai_thumbnail', { aiId: oldId });
            await invoke('save_ai_thumbnail', { aiId: agentKey, thumbnailData: thumbData });
            await invoke('delete_ai_thumbnail', { aiId: oldId });
          } catch { /* no thumbnail */ }
          console.log(`[AiDataContext] Agent provisioned for new AI: ${newAi.name} (${agentKey.slice(0, 16)}...)`);
        }
      } catch (e) {
        console.warn("[AiDataContext] Agent provisioning for new AI failed:", e);
      }

      state.userDefinedAis = [...state.userDefinedAis, newAi];
      return newAi;
    }
  );

  const updateCustomAi = $(
    async (
      id: string,
      updates: Partial<UserDefinedAI>
    ): Promise<UserDefinedAI> => {
      const updatedAi = await updateLocalCustomAi(id, updates);
      if (!updatedAi) throw new Error("AI not found");
      state.userDefinedAis = state.userDefinedAis.map((ai) =>
        ai.id === id ? updatedAi : ai
      );
      return updatedAi;
    }
  );

  const deleteCustomAi = $(async (id: string): Promise<boolean> => {
    const success = await deleteLocalCustomAi(id);
    if (success) {
      state.userDefinedAis = state.userDefinedAis.filter(
        (ai) => ai.id !== id
      );
    }
    return success;
  });

  /**
   * Archive an AI: retire it from the active list but keep everything —
   * its config, agent key, and the signed transcripts on the conductor.
   * Reversible via restoreCustomAi.
   */
  const archiveCustomAi = $(async (id: string): Promise<UserDefinedAI> => {
    const updatedAi = await updateLocalCustomAi(id, {
      status: "archived",
      archivedAt: Date.now(),
    });
    if (!updatedAi) throw new Error("AI not found");
    state.userDefinedAis = state.userDefinedAis.map((ai) =>
      ai.id === id ? updatedAi : ai
    );
    return updatedAi;
  });

  /** Bring an archived AI back to active. */
  const restoreCustomAi = $(async (id: string): Promise<UserDefinedAI> => {
    const updatedAi = await updateLocalCustomAi(id, {
      status: "active",
      archivedAt: undefined,
    });
    if (!updatedAi) throw new Error("AI not found");
    state.userDefinedAis = state.userDefinedAis.map((ai) =>
      ai.id === id ? updatedAi : ai
    );
    return updatedAi;
  });

  /**
   * Purge an AI: irreversibly delete it and everything it produced — the
   * hApp + signed transcripts on the conductor, the memory facts it taught,
   * its thumbnail, and the local config. hApp uninstall is best-effort
   * (the conductor may be down); local cleanup proceeds regardless.
   */
  const purgeCustomAi = $(async (id: string): Promise<boolean> => {
    try {
      await invoke("purge_ai_app", { aiId: id });
    } catch (e) {
      console.warn(
        "[AiDataContext] purge_ai_app failed (continuing with local cleanup):",
        e
      );
    }
    try {
      const { forgetAll } = await import("../utils/memory");
      await forgetAll(id);
    } catch (e) {
      console.warn("[AiDataContext] forgetAll failed:", e);
    }
    try {
      await invoke("delete_ai_thumbnail", { aiId: id });
    } catch {
      /* no thumbnail */
    }
    const success = await deleteLocalCustomAi(id);
    if (success) {
      state.userDefinedAis = state.userDefinedAis.filter(
        (ai) => ai.id !== id
      );
    }
    return success;
  });

  const updateCustomAiStatus = $(
    async (
      id: string,
      status: "active" | "inactive"
    ): Promise<UserDefinedAI> => {
      const updatedAi = await updateLocalCustomAiStatus(id, status);
      if (!updatedAi) throw new Error("AI not found");
      state.userDefinedAis = state.userDefinedAis.map((ai) =>
        ai.id === id ? updatedAi : ai
      );
      return updatedAi;
    }
  );

  const addUserAi = $(
    async (data: CreateUserAiData): Promise<UserDefinedAI | null> => {
      try {
        const newAi = await saveLocalCustomAi({
          name: data.name,
          baseArchetypeId: data.baseArchetypeId,
          systemPrompt: data.systemPrompt,
          description: data.description,
          lengthDisposition: data.lengthDisposition,
          defaultMode: data.defaultMode,
          model: data.model,
          emoji: data.emoji,
        });

        // Provision the Holochain agent NOW (and adopt its pub key as the
        // canonical ID) so transcripts record from the very first turn and the
        // Memory page works immediately. Without this the AI has no
        // agentPubKey until the next provisioning sweep, so its early
        // conversations aren't recorded (the "new AI doesn't work straight
        // away" bug).
        try {
          const { provisionAgent } = await import("../utils/holochainTranscripts");
          const agentKey = await provisionAgent(newAi.id);
          if (agentKey) {
            const oldId = newAi.id;
            newAi.agentPubKey = agentKey;
            newAi.id = agentKey;
            await updateLocalCustomAi(oldId, { id: agentKey, agentPubKey: agentKey } as any);
            try {
              const thumbData = await invoke<number[]>("get_ai_thumbnail", { aiId: oldId });
              await invoke("save_ai_thumbnail", { aiId: agentKey, thumbnailData: thumbData });
              await invoke("delete_ai_thumbnail", { aiId: oldId });
            } catch { /* no thumbnail */ }
            console.log(`[AiDataContext] Agent provisioned for new AI: ${newAi.name} (${agentKey.slice(0, 16)}...)`);
          }
        } catch (e) {
          console.warn("[AiDataContext] Agent provisioning for new AI failed:", e);
        }

        state.userDefinedAis = [...state.userDefinedAis, newAi];
        return newAi;
      } catch (error) {
        console.error("[AiDataContext] Error in addUserAi:", error);
        state.userAisError =
          error instanceof Error ? error.message : "Failed to create AI";
        return null;
      }
    }
  );

  const editUserAi = $(
    async (
      id: string,
      data: UpdateUserAiData
    ): Promise<UserDefinedAI | null> => {
      try {
        const updatedAi = await updateLocalCustomAi(id, data);
        if (!updatedAi) throw new Error("AI not found");
        state.userDefinedAis = state.userDefinedAis.map((ai) =>
          ai.id === id ? updatedAi : ai
        );
        return updatedAi;
      } catch (error) {
        console.error("[AiDataContext] Error in editUserAi:", error);
        state.userAisError =
          error instanceof Error ? error.message : "Failed to update AI";
        return null;
      }
    }
  );

  const updateAllAisWithFirstModel = $(async (filename: string) => {
    console.log(
      "[AiDataContext] Updating all AIs with first model:",
      filename
    );

    for (const ai of state.userDefinedAis) {
      const updatedAi = await updateLocalCustomAi(ai.id, {
        model: filename,
      });
      if (updatedAi) {
        state.userDefinedAis = state.userDefinedAis.map((a) =>
          a.id === ai.id ? updatedAi : a
        );
      }
    }
  });

  const refreshUserAis = $(async () => {
    state.isLoadingUserAis = true;
    try {
      const localAis = await getLocalCustomAis();
      state.userDefinedAis = localAis;
    } catch (error) {
      console.error("[AiDataContext] Error refreshing user AIs:", error);
    } finally {
      state.isLoadingUserAis = false;
    }
  });

  /** Reload a single AI's thumbnail from disk into thumbnailObjectUrls */
  const refreshThumbnail = $(async (aiId: string) => {
    try {
      const thumbnailData = await invoke<number[]>("get_ai_thumbnail", { aiId });
      const uint8Array = new Uint8Array(thumbnailData);
      const blob = new Blob([uint8Array], { type: "image/jpeg" });
      // Revoke old URL to avoid memory leaks
      const oldUrl = state.thumbnailObjectUrls[aiId];
      if (oldUrl && oldUrl.startsWith("blob:")) {
        URL.revokeObjectURL(oldUrl);
      }
      state.thumbnailObjectUrls = {
        ...state.thumbnailObjectUrls,
        [aiId]: URL.createObjectURL(blob),
      };
    } catch {
      // No custom thumbnail on disk — remove from map if it was there
      if (state.thumbnailObjectUrls[aiId]) {
        const { [aiId]: _, ...rest } = state.thumbnailObjectUrls;
        state.thumbnailObjectUrls = rest;
      }
    }
  });

  /** Reorder AIs and persist to store */
  const reorderAis = $(async (reorderedAis: UserDefinedAI[]) => {
    state.userDefinedAis = reorderedAis;
    // Persist the new order by overwriting the store
    try {
      const { Store } = await import("@tauri-apps/plugin-store");
      const store = await Store.load("ai-data.json");
      await store.set("custom-ais", reorderedAis);
      await store.save();
    } catch (error) {
      console.error("[AiDataContext] Error persisting AI order:", error);
    }
  });

  return {
    createCustomAi,
    updateCustomAi,
    deleteCustomAi,
    archiveCustomAi,
    restoreCustomAi,
    purgeCustomAi,
    updateCustomAiStatus,
    addUserAi,
    editUserAi,
    updateAllAisWithFirstModel,
    refreshUserAis,
    refreshThumbnail,
    reorderAis,
  };
}
