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
import { prefetchModels } from "../utils/modelCache";

export type AppTheme = "light" | "dark";

export const ThemeContext = createContextId<{
  theme: Signal<AppTheme>;
}>("app.theme");

export default component$(() => {
  const theme = useSignal<AppTheme>("dark");

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

  return (
    <ModeProvider>
      <AiDataProvider>
        <VisionDownloadProvider>
          <Slot />
          <VisionDownloadIndicator />
        </VisionDownloadProvider>
      </AiDataProvider>
    </ModeProvider>
  );
});
