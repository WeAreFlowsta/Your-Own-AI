/**
 * First-model finisher (root-level, always mounted).
 *
 * While the welcome wizard's first download runs, the activity tray shows
 * its row everywhere but the wizard itself; this component OWNS the finish: when the file lands it loads the model, assigns
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
import { announceActivity } from "../utils/activity";
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
      announceActivity({ id: "first-model", title: `${f.label} is ready`, detail: "Your AIs can answer now.", state: "done", ttlMs: 15000 });
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

  // The activity tray draws the download row and the "is ready" note; this
  // component only owns the finish (load + assign + announce).
  void loc;
  void percent;
  void readyLabel;
  return null;
});
