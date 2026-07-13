/**
 * Dev Server Entry Point
 *
 * Used during `npm run dev` / `tauri dev` for hot-reload development.
 * In production, the static adapter (entry.ssr.tsx) is used instead.
 */
import { type RenderOptions, render } from "@builder.io/qwik";
import Root from "./root";

export default function (opts: RenderOptions) {
  return render(document, <Root />, opts);
}
