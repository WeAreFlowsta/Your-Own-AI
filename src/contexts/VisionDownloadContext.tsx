/**
 * App-level vision-model download manager. Lives in the root layout so a long
 * (multi-GB) download survives navigating between pages: the reactive store and
 * the running download loop both persist because the layout never unmounts on
 * route changes. A small global indicator reads this store; the chat page picks
 * up the completion to auto-resume the message that triggered the download.
 *
 * Reload survival: the active download's descriptor (not the queued message) is
 * mirrored to localStorage, so an app restart mid-download re-invokes
 * download_model — which resumes from the on-disk `.part` — and re-shows the
 * indicator. The queued message is only restored within the same session.
 */
import {
  component$,
  createContextId,
  useContext,
  useContextProvider,
  useStore,
  useVisibleTask$,
  $,
  Slot,
  type QRL,
} from "@builder.io/qwik";
import { modelManager } from "../utils/modelManager";
import type { ChatAction } from "../types";

export interface VisionPlanFile {
  url: string;
  filename: string;
  label: string;
  size: number; // GB
}

export interface VisionPendingTurn {
  userInput: string;
  chatAction: ChatAction;
  fileContext?: string;
  images: string[];
}

export interface ActiveVisionDownload {
  files: VisionPlanFile[];
  currentIndex: number;
  percent: number;
  downloaded: number;
  total: number;
  status: "downloading" | "done" | "error";
  error: string | null;
  visionModel: string;
  aiId: string;
  pendingTurn: VisionPendingTurn | null;
}

export interface VisionReady {
  visionModel: string;
  pendingTurn: VisionPendingTurn | null;
}

export interface VisionDownloadState {
  active: ActiveVisionDownload | null;
}

export interface VisionDownloadApi {
  state: VisionDownloadState;
  start$: QRL<
    (
      files: VisionPlanFile[],
      visionModel: string,
      aiId: string,
      pendingTurn: VisionPendingTurn | null,
    ) => void
  >;
  retry$: QRL<() => void>;
  dismiss$: QRL<() => void>;
  consumeReady$: QRL<(aiId: string) => VisionReady | null>;
}

export const VisionDownloadContext =
  createContextId<VisionDownloadApi>("app.visiondownload");

const STORE_KEY = "activeVisionDownload";

export const VisionDownloadProvider = component$(() => {
  const state = useStore<VisionDownloadState>({ active: null }, { deep: true });

  // Persist just enough to resume after an app restart (not the queued message,
  // which can hold a large image — that stays in-session).
  const persist = $(() => {
    const a = state.active;
    if (!a || a.status === "done") {
      localStorage.removeItem(STORE_KEY);
      return;
    }
    localStorage.setItem(
      STORE_KEY,
      JSON.stringify({
        files: a.files,
        currentIndex: a.currentIndex,
        visionModel: a.visionModel,
        aiId: a.aiId,
      }),
    );
  });

  // The download loop. Mutates `state.active` so the global indicator tracks it.
  const run$ = $(async (active: ActiveVisionDownload) => {
    state.active = active;
    await persist();
    try {
      for (let i = active.currentIndex; i < active.files.length; i++) {
        if (state.active) {
          state.active.currentIndex = i;
          state.active.percent = 0;
        }
        await persist();
        try {
          await modelManager.downloadModel(
            active.files[i].url,
            active.files[i].filename,
            (p) => {
              if (state.active) {
                state.active.percent = p.percent;
                state.active.downloaded = p.downloaded;
                state.active.total = p.total;
              }
            },
          );
        } catch (e) {
          // Already on disk (e.g. resumed after the file finished) — treat as done.
          if (!String(e).toLowerCase().includes("already downloaded")) throw e;
        }
      }
      if (state.active) {
        state.active.status = "done";
        state.active.percent = 100;
      }
      await persist();
    } catch (e) {
      if (state.active) {
        state.active.status = "error";
        state.active.error = e instanceof Error ? e.message : String(e);
      }
      await persist();
    }
  });

  const start$ = $(
    (
      files: VisionPlanFile[],
      visionModel: string,
      aiId: string,
      pendingTurn: VisionPendingTurn | null,
    ) => {
      // Already downloading (the vision model is the same regardless of which AI
      // asked) → don't spawn a second loop; just point the resume at this latest
      // request so the right message continues when it finishes.
      if (state.active && state.active.status === "downloading") {
        state.active.aiId = aiId;
        state.active.pendingTurn = pendingTurn;
        return;
      }
      run$({
        files,
        currentIndex: 0,
        percent: 0,
        downloaded: 0,
        total: 0,
        status: "downloading",
        error: null,
        visionModel,
        aiId,
        pendingTurn,
      });
    },
  );

  const retry$ = $(() => {
    const a = state.active;
    if (!a) return;
    run$({ ...a, status: "downloading", error: null });
  });

  const dismiss$ = $(() => {
    state.active = null;
    localStorage.removeItem(STORE_KEY);
  });

  // Hand the completed download (+ queued message) to the chat page for the
  // matching AI, then clear it so it fires once.
  const consumeReady$ = $((aiId: string): VisionReady | null => {
    const a = state.active;
    if (!a || a.status !== "done" || a.aiId !== aiId) return null;
    const ready: VisionReady = {
      visionModel: a.visionModel,
      pendingTurn: a.pendingTurn,
    };
    state.active = null;
    localStorage.removeItem(STORE_KEY);
    return ready;
  });

  // Resume an interrupted download after an app restart.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    const raw = localStorage.getItem(STORE_KEY);
    if (!raw) return;
    try {
      const d = JSON.parse(raw);
      if (!d?.files?.length) {
        localStorage.removeItem(STORE_KEY);
        return;
      }
      run$({
        files: d.files,
        currentIndex: d.currentIndex ?? 0,
        percent: 0,
        downloaded: 0,
        total: 0,
        status: "downloading",
        error: null,
        visionModel: d.visionModel,
        aiId: d.aiId,
        pendingTurn: null, // not restored across a restart
      });
    } catch {
      localStorage.removeItem(STORE_KEY);
    }
  });

  useContextProvider(VisionDownloadContext, {
    state,
    start$,
    retry$,
    dismiss$,
    consumeReady$,
  });

  return <Slot />;
});

export function useVisionDownload() {
  return useContext(VisionDownloadContext);
}
