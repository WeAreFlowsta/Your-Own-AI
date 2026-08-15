import {
  component$,
  createContextId,
  Slot,
  useContextProvider,
  useSignal,
  useVisibleTask$,
  type Signal,
} from "@builder.io/qwik";
import { ModeProvider } from "../contexts/ModeContext";
import { AiDataProvider } from "../contexts/AiDataContext";
import { VisionDownloadProvider } from "../contexts/VisionDownloadContext";
import { VisionDownloadIndicator } from "../components/VisionDownloadIndicator";
import { WorkspaceMemoryModal } from "../components/WorkspaceMemoryModal";
import { prefetchModels } from "../utils/modelCache";
import { mirrorPausedModels } from "../utils/modelPrefs";

export type AppTheme = "light" | "dark";

export const ThemeContext = createContextId<{
  theme: Signal<AppTheme>;
}>("app.theme");

/** The project-memory modal's control: set a folder path to open it. The
 *  modal itself renders HERE at the layout root - every route wraps its
 *  content (and the header) in its own stacking contexts, so a modal
 *  rendered inside them can never layer above the page. */
export const ProjectMemoryContext = createContextId<Signal<string | null>>(
  "app.project-memory",
);

export default component$(() => {
  const theme = useSignal<AppTheme>("dark");

  // Routing picks predating the Settings store-mirror live only in
  // localStorage, which HTTP-path routing (agent sessions) can't see.
  // Mirror them into the tauri store once per launch so old picks apply
  // everywhere. Best-effort; the Settings page keeps both in sync from
  // here on.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    (async () => {
      try {
        const { Store } = await import("@tauri-apps/plugin-store");
        const store = await Store.load("settings.json");
        let dirty = false;
        for (const key of [
          "routingOnlineAgent",
          "routingOnlinePlanning",
          "routingOnlineHardCode",
          "routingOnlineHardGeneral",
          "routingOnlineFresh",
          "routingProjectThrifty",
          "routingProjectDeviceSubagents",
        ]) {
          const local = localStorage.getItem(key);
          if (local !== null && (await store.get(key)) == null) {
            await store.set(key, local);
            dirty = true;
          }
        }
        if (dirty) await store.save();
      } catch {
        /* store unavailable - picks stay webview-only */
      }
    })();

    // Resume an interrupted history-import distill (cursor survives
    // restarts; no-op when nothing is pending). Delayed so it never
    // competes with the busy startup window.
    setTimeout(() => {
      import("../utils/importDistiller")
        .then((m) => m.resumeIfPending())
        .catch(() => {});
    }, 15_000);

    // Finish any adoption (imported archive -> AI's conversations) the
    // last run left mid-write. Waits for the records engine itself;
    // no-op when nothing is pending.
    setTimeout(() => {
      import("../utils/importAdoption")
        .then((m) => m.resumeIfPending())
        .catch(() => {});
    }, 20_000);

    // One-time repair: restore facts left hidden by forgets that removed
    // their superseding fact before restore-on-forget existed.
    setTimeout(() => {
      const FLAG = "memory-orphan-repair-v1";
      if (localStorage.getItem(FLAG)) return;
      import("../utils/memory")
        .then(async (m) => {
          await m.repairMemoryOrphans();
          localStorage.setItem(FLAG, "done");
        })
        .catch(() => {});
    }, 25_000);
  });

  // Load saved theme from localStorage on mount + dismiss loading overlay + global liquid metal hover
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const saved = localStorage.getItem("theme");
    if (saved === "light" || saved === "dark") {
      theme.value = saved;
    }

    // Warm the model-picker cache (local models + fit grades + online catalog) so
    // the first Edit-AI dropdown open is instant instead of cold-reading GGUF headers.
    prefetchModels();
    // Sync pre-existing pauses into the store the router reads - pauses
    // made before the router honored them must count from this launch on.
    void mirrorPausedModels();

    // Loading overlay is dismissed by AiDataContext after full initialization
    // (archetypes + AIs + agent provisioning + thumbnails)

    // Global liquid metal shader hover manager
    // Creates one ShaderMount at a time, disposes on leave
    let activeMount: any = null;
    let activeShaderEl: HTMLElement | null = null;
    let shaderModule: any = null;

    const loadShaderModule = async () => {
      if (!shaderModule) {
        shaderModule = await import("@paper-design/shaders");
      }
      return shaderModule;
    };

    const activateShader = async (shaderEl: HTMLElement) => {
      if (activeShaderEl === shaderEl) return;
      deactivateShader();

      const mod = await loadShaderModule();
      const { ShaderMount, liquidMetalFragmentShader, getShaderColorFromString, LiquidMetalShapes, ShaderFitOptions, defaultObjectSizing } = mod;

      const isDark = !document.documentElement.classList.contains("theme-light");
      const colors = isDark
        ? { back: "#B8B0A4", tint: "#F0E8E0" }
        : { back: "#686070", tint: "#787080" };

      activeMount = new ShaderMount(shaderEl, liquidMetalFragmentShader, {
        u_colorBack: getShaderColorFromString(colors.back),
        u_colorTint: getShaderColorFromString(colors.tint),
        u_image: undefined,
        u_imageAspectRatio: 1,
        u_repetition: 6,
        u_shiftRed: 0.9,
        u_shiftBlue: 0.7,
        u_contour: 0.5,
        u_softness: 0.5,
        u_distortion: 0.3,
        u_angle: 30,
        u_isImage: false,
        u_shape: LiquidMetalShapes["none"],
        u_fit: ShaderFitOptions[defaultObjectSizing.fit],
        u_scale: 1.5,
        u_rotation: defaultObjectSizing.rotation,
        u_offsetX: defaultObjectSizing.offsetX,
        u_offsetY: defaultObjectSizing.offsetY,
        u_originX: defaultObjectSizing.originX,
        u_originY: defaultObjectSizing.originY,
        u_worldWidth: 0,
        u_worldHeight: 0,
      }, undefined, 1);

      shaderEl.style.opacity = "1";
      activeShaderEl = shaderEl;
    };

    const deactivateShader = () => {
      if (activeMount) {
        activeMount.dispose();
        activeMount = null;
      }
      if (activeShaderEl) {
        activeShaderEl.style.opacity = "0";
        const canvas = activeShaderEl.querySelector("canvas");
        if (canvas) canvas.remove();
        activeShaderEl = null;
      }
    };

    const onMouseOver = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      const container = target.closest(".btn-liquid-metal, .liquid-metal-border");
      if (!container) {
        deactivateShader();
        return;
      }
      if ((container as HTMLElement).hasAttribute("disabled")) return;
      const shaderEl = container.querySelector(".shader-border, .liquid-metal-border-shader") as HTMLElement;
      if (shaderEl) {
        activateShader(shaderEl);
      }
    };

    const onMouseOut = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      const container = target.closest(".btn-liquid-metal, .liquid-metal-border");
      if (!container) return;
      const related = e.relatedTarget as HTMLElement | null;
      if (related && container.contains(related)) return;
      deactivateShader();
    };

    document.addEventListener("mouseover", onMouseOver);
    document.addEventListener("mouseout", onMouseOut);

    cleanup(() => {
      document.removeEventListener("mouseover", onMouseOver);
      document.removeEventListener("mouseout", onMouseOut);
      deactivateShader();
    });
  });

  // Apply theme to DOM and persist whenever it changes
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const current = track(() => theme.value);

    const root = document.documentElement;
    const body = document.body;

    root.classList.remove("theme-light", "theme-dark");
    root.classList.add(`theme-${current}`);

    body.classList.remove("theme-light", "theme-dark");
    body.classList.add(`theme-${current}`);

    localStorage.setItem("theme", current);
  });

  useContextProvider(ThemeContext, { theme });

  const projectMemoryFolder = useSignal<string | null>(null);
  useContextProvider(ProjectMemoryContext, projectMemoryFolder);

  return (
    <ModeProvider>
      <AiDataProvider>
        <VisionDownloadProvider>
          <Slot />
          <VisionDownloadIndicator />
          {/* Root-level so no route stacking context can bury it. */}
          <WorkspaceMemoryModal folderPath={projectMemoryFolder} />
        </VisionDownloadProvider>
      </AiDataProvider>
    </ModeProvider>
  );
});
