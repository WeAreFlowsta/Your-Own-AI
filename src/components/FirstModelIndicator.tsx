/**
 * First-model notice + finisher (root-level, always mounted).
 *
 * While the welcome wizard's first download runs, this shows a small
 * "Downloading your first model" card everywhere but the wizard itself,
 * and it OWNS the finish: when the file lands it loads the model, assigns
 * it to every AI, clears the in-flight state and announces
 * FIRST_MODEL_READY (the chat answers its held question on that). Doing
 * this here means it happens whether the user stayed on the wizard, moved
 * to the chat, or restarted the app mid-download (the engine resumes the
 * .part through the same download call).
 */

import { component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { useLocation } from "@builder.io/qwik-city";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAiDataActions } from "../contexts/AiDataContext";
import { modelFamilies } from "../data/recommended-models";
import { modelManager, type DownloadProgress } from "../utils/modelManager";
import {
  firstModelInFlight,
  clearFirstModelInFlight,
  FIRST_MODEL_CHANGED,
  FIRST_MODEL_READY,
  type FirstModelInFlight,
} from "../utils/firstModel";

function downloadUrlFor(filename: string): string | null {
  for (const f of modelFamilies) {
    for (const v of f.variants) {
      if (v.filename === filename) return v.downloadUrl;
    }
  }
  return null;
}

export const FirstModelIndicator = component$(() => {
  const loc = useLocation();
  const { updateAllAisWithFirstModel } = useAiDataActions();
  const inFlight = useSignal<FirstModelInFlight | null>(null);
  const percent = useSignal<number | null>(null);
  const readyLabel = useSignal<string | null>(null);
  const finishing = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    inFlight.value = firstModelInFlight();

    const finish = async (f: FirstModelInFlight) => {
      if (finishing.value) return;
      finishing.value = true;
      try {
        await invoke("load_model", { filename: f.filename, withVision: false, reason: "welcome" });
      } catch (e) {
        console.error("[FirstModel] load failed:", e);
      }
      try {
        await updateAllAisWithFirstModel(f.filename);
      } catch (e) {
        console.error("[FirstModel] assigning the model to the AIs failed:", e);
      }
      clearFirstModelInFlight();
      readyLabel.value = f.label;
      finishing.value = false;
      window.dispatchEvent(new CustomEvent(FIRST_MODEL_READY, { detail: { filename: f.filename, label: f.label } }));
    };

    const onChanged = () => {
      inFlight.value = firstModelInFlight();
      if (inFlight.value) {
        percent.value = null;
        readyLabel.value = null;
      }
    };
    window.addEventListener(FIRST_MODEL_CHANGED, onChanged);

    const unProgress = await listen<DownloadProgress>("model-download-progress", (e) => {
      const f = firstModelInFlight();
      if (f && e.payload.filename === f.filename) percent.value = e.payload.percent;
    });
    const unDone = await listen<{ filename: string }>("model-download-complete", (e) => {
      const f = firstModelInFlight();
      if (f && e.payload.filename === f.filename) void finish(f);
    });

    // Picking up after a restart: the file may have landed while the app
    // was closed (finish now), or be a .part the engine is not running any
    // more (resume it - the same call continues from where it stopped).
    const f = firstModelInFlight();
    if (f) {
      try {
        const st = await modelManager.downloadStatus(f.filename);
        if (!st.downloading) {
          if (st.has_partial) {
            const url = downloadUrlFor(f.filename);
            if (url) void modelManager.downloadModel(url, f.filename).catch((e) => console.warn("[FirstModel] resume failed:", e));
          } else {
            const models = await invoke<{ filename?: string; name?: string }[]>("list_local_models");
            if (models.some((m) => (m.filename ?? m.name) === f.filename)) void finish(f);
          }
        }
      } catch (e) {
        console.warn("[FirstModel] status check failed:", e);
      }
    }

    cleanup(() => {
      unProgress();
      unDone();
      window.removeEventListener(FIRST_MODEL_CHANGED, onChanged);
    });
  });

  // The wizard shows its own progress; the notice is for every other page.
  if (loc.url.pathname.startsWith("/welcome")) return null;
  if (!inFlight.value && !readyLabel.value) return null;

  return (
    <div class="fixed bottom-4 right-4 z-[60] w-72 max-w-[calc(100vw-2rem)] rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] shadow-lg p-3">
      {readyLabel.value ? (
        <div class="flex items-start justify-between gap-2">
          <div>
            <p class="text-sm font-medium text-[var(--text-primary)]">{readyLabel.value} is ready</p>
            <p class="text-xs text-[var(--text-muted)] mt-0.5">Your AIs can answer now.</p>
          </div>
          <button
            type="button"
            class="text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
            onClick$={() => {
              readyLabel.value = null;
            }}
          >
            Dismiss
          </button>
        </div>
      ) : (
        <>
          <div class="flex items-center gap-2 mb-2">
            <span class="inline-block w-2 h-2 rounded-full bg-[var(--bg-button-primary)] animate-pulse" />
            <span class="text-sm font-medium text-[var(--text-primary)]">Downloading your first model</span>
          </div>
          <p class="text-xs text-[var(--text-muted)] mb-2 truncate">{inFlight.value?.label}</p>
          <div class="w-full h-2 rounded-full bg-[var(--bg-main)] overflow-hidden">
            <div class="h-full bg-[var(--bg-button-primary)] transition-all duration-200" style={{ width: `${percent.value ?? 0}%` }} />
          </div>
          <p class="text-xs text-[var(--text-muted)] mt-1">
            {percent.value !== null ? `${percent.value}%` : "Starting.."} · your AIs answer the moment it lands
          </p>
        </>
      )}
    </div>
  );
});
