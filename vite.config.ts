import { defineConfig, type UserConfig } from "vite";
import { qwikVite } from "@builder.io/qwik/optimizer";
import { qwikCity } from "@builder.io/qwik-city/vite";
import tsconfigPaths from "vite-tsconfig-paths";

const host = process.env.TAURI_DEV_HOST;

// Tauri expects a fixed port
const TAURI_DEV_PORT = 5173;

export default defineConfig((): UserConfig => ({
  plugins: [
    qwikCity(),
    qwikVite(),
    tsconfigPaths({ root: "." }),
  ],

  // prevent Vite from obscuring rust errors
  clearScreen: false,

  // Pre-bundle every runtime dependency at server start. beforeDevCommand
  // wipes node_modules/.vite on each run (needed to avoid stale-SSR
  // Code(14) errors — see qwik-pitfalls.md), which otherwise makes Vite
  // discover these lazily DURING the first page load and then force a
  // full reload ("✨ optimized dependencies changed. reloading") mid-
  // hydration — the intermittent header-only/stuck first page in dev.
  // Listing them here moves optimization before the first request, so
  // the mid-flight reload can never fire. If you add a new runtime dep
  // and see the "optimized dependencies changed" line again, add it here.
  optimizeDeps: {
    include: [
      "@tauri-apps/api/core",
      "@tauri-apps/api/app",
      "@flowsta/login-button",
      "@tauri-apps/api/event",
      "@tauri-apps/api/webviewWindow",
      "@tauri-apps/plugin-store",
      "@tauri-apps/plugin-dialog",
      "@tauri-apps/plugin-opener",
      "@paper-design/shaders",
      "highlight.js",
      "uuid",
      "marked",
      "marked-highlight",
      "lottie-web",
      "cropperjs",
    ],
  },

  build: {
    target: "esnext",
    outDir: "dist",
  },

  // Tauri expects a fixed port, fail if that port is not available
  server: {
    port: TAURI_DEV_PORT,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: TAURI_DEV_PORT + 1,
        }
      : undefined,
    watch: {
      // tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
