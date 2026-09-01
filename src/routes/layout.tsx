import {
  component$,
  createContextId,
  Slot,
  useContextProvider,
  useSignal,
  useVisibleTask$,
  type Signal,
} from "@builder.io/qwik";
import { useNavigate } from "@builder.io/qwik-city";
import { ModeProvider } from "../contexts/ModeContext";
import { AiDataProvider } from "../contexts/AiDataContext";
import { VisionDownloadProvider } from "../contexts/VisionDownloadContext";
import { VisionDownloadIndicator } from "../components/VisionDownloadIndicator";
import { WorkspaceMemoryModal } from "../components/WorkspaceMemoryModal";
import ConfirmModal from "../components/ConfirmModal";
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
    // Same for the attachments-online consent: an existing "on" must reach
    // the router's store from this launch on.
    void (async () => {
      try {
        const on = localStorage.getItem("allowAttachmentsOnline") === "true";
        const { Store } = await import("@tauri-apps/plugin-store");
        const store = await Store.load("settings.json");
        await store.set("allowAttachmentsOnline", on);
        await store.save();
      } catch { /* best-effort */ }
    })();

    // The moment a plan activates: record the entitlement once at launch
    // (after startup has settled) and re-check on focus/visibility while
    // the last known state is not-entitled - a plan is bought in the
    // browser, so coming back to the app IS the moment. No polling once
    // entitled; the check is throttled and best-effort.
    const launchCheck = setTimeout(() => {
      import("../utils/entitlement")
        .then(({ getOnlineEntitlement }) => getOnlineEntitlement())
        .catch(() => { /* best-effort */ });
    }, 6000);
    const recheckEntitlement = () => {
      if (document.visibilityState === "hidden") return;
      import("../utils/entitlement")
        .then(({ recheckEntitlementIfUnentitled }) => recheckEntitlementIfUnentitled())
        .catch(() => { /* best-effort */ });
    };
    window.addEventListener("focus", recheckEntitlement);
    document.addEventListener("visibilitychange", recheckEntitlement);
    cleanup(() => {
      clearTimeout(launchCheck);
      window.removeEventListener("focus", recheckEntitlement);
      document.removeEventListener("visibilitychange", recheckEntitlement);
    });

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

  // UI-stall watchdog. CSS spinners animate on the compositor thread, so
  // the app can look alive while the main thread - which handles every
  // click - is frozen; "the page ignored me" field reports can't tell a
  // freeze from slow handlers. Record main-thread gaps so diagnostics
  // can: console.warn + the last 20 in localStorage("uiStalls").
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    let last = performance.now();
    let raf = 0;
    const tick = () => {
      const now = performance.now();
      const gap = now - last;
      // Hidden windows throttle rAF legitimately - not a stall.
      if (gap > 1000 && !document.hidden) {
        console.warn(`[ui] main thread stalled ${Math.round(gap)}ms on ${location.pathname}`);
        try {
          const prev = JSON.parse(localStorage.getItem("uiStalls") || "[]");
          prev.push({ ms: Math.round(gap), at: Date.now(), path: location.pathname });
          localStorage.setItem("uiStalls", JSON.stringify(prev.slice(-20)));
        } catch {
          /* diagnostics only - never let the watchdog hurt the app */
        }
      }
      last = now;
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    cleanup(() => cancelAnimationFrame(raf));
  });

  // yourownai:// links ("Add to Your Own AI" on the site). The path maps
  // onto the app's own routes; a skills link carries the source to add.
  const nav = useNavigate();

  // A turn in flight lives in the chat page's listeners; leaving the page
  // mid-turn loses its answer (the agent finishes, nothing records it).
  // Until the session is lifted to app level, hold in-app link clicks with
  // a question while the chat hook says a turn is running.
  const leaveAsk = useSignal<string | null>(null);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const onClick = (e: MouseEvent) => {
      const running = (window as unknown as { __yoaiTurnRunning?: boolean }).__yoaiTurnRunning;
      if (!running) return;
      const a = (e.target as HTMLElement | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
      if (!a) return;
      const href = a.getAttribute("href") || "";
      if (!href.startsWith("/") || href.startsWith("//")) return; // external links open elsewhere
      if (href.replace(/\/$/, "") === window.location.pathname.replace(/\/$/, "")) return;
      e.preventDefault();
      e.stopPropagation();
      leaveAsk.value = href;
    };
    document.addEventListener("click", onClick, true);
    cleanup(() => document.removeEventListener("click", onClick, true));
  });
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    try {
      const { listen } = await import("@tauri-apps/api/event");
      const un = await listen<string>("deep-link", async (e) => {
        const raw = typeof e.payload === "string" ? e.payload : "";
        if (!raw.startsWith("yourownai://")) return;
        let path = "/";
        let link = "";
        let id = "";
        try {
          const u = new URL(raw.replace(/^yourownai:\/\//, "https://app.local/"));
          path = u.pathname;
          link = u.searchParams.get("link") ?? "";
          id = u.searchParams.get("id") ?? "";
        } catch {
          return;
        }
        if (!/^\/(add-ons|your-ais|your-memory|chat|settings|online-models|setup)(\/|$)/.test(path)) return;
        if (path.startsWith("/add-ons/skills") && link) {
          try {
            sessionStorage.setItem("skillsAddLink", link);
          } catch {
            /* the page still opens; the user pastes the link */
          }
        }
        // A character or tool link names its add-on: the page scrolls to
        // that card and rings it. Handed over the same way as skills.
        if ((path.startsWith("/add-ons/characters") || path.startsWith("/add-ons/mcp")) && id) {
          try {
            sessionStorage.setItem("addOnFocusId", id);
          } catch {
            /* the page still opens; the card is a scroll away */
          }
        }
        await nav(path);
      });
      cleanup(() => un());
    } catch {
      /* not inside Tauri */
    }
  });

  return (
    <ModeProvider>
      <AiDataProvider>
        <VisionDownloadProvider>
          <Slot />
          <VisionDownloadIndicator />
          {/* Root-level so no route stacking context can bury it. */}
          <WorkspaceMemoryModal folderPath={projectMemoryFolder} />
          <ConfirmModal
            isOpen={leaveAsk.value !== null}
            title="Your AI is mid-turn"
            message="If you leave this page now, the turn keeps running but its answer will not be saved to the conversation. Wait for it to finish, or leave anyway."
            confirmLabel="Leave anyway"
            cancelLabel="Stay"
            variant="danger"
            onConfirm$={async () => {
              const href = leaveAsk.value;
              leaveAsk.value = null;
              try { (window as unknown as { __yoaiTurnRunning?: boolean }).__yoaiTurnRunning = false; } catch { /* fine */ }
              if (href) await nav(href);
            }}
            onCancel$={() => { leaveAsk.value = null; }}
          />
        </VisionDownloadProvider>
      </AiDataProvider>
    </ModeProvider>
  );
});
