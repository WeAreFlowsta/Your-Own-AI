/**
 * Which view an agent rail shows: "simple" (the story: folded families,
 * no thoughts) or "detailed" (every row, thought and live line as it
 * happens). One setting per SURFACE - projects default to Detailed, chat
 * with tools to Simple (Eric, 2026-09-23) - sticky in localStorage, and
 * every open rail follows a change at once through a window event.
 */
export type AgentSurface = "project" | "tools";
export type AgentView = "simple" | "detailed";

export const AGENT_VIEW_EVENT = "yoai-agent-view";
const KEY: Record<AgentSurface, string> = { project: "agent-view-project", tools: "agent-view-tools" };
const DEFAULT: Record<AgentSurface, AgentView> = { project: "detailed", tools: "simple" };
/** The pre-0.8.0 key ("0" = simple for everything). Read once as a hint. */
const LEGACY_KEY = "agent-show-thoughts";

export function getAgentView(surface: AgentSurface): AgentView {
  try {
    const v = localStorage.getItem(KEY[surface]);
    if (v === "simple" || v === "detailed") return v;
    if (surface === "project" && localStorage.getItem(LEGACY_KEY) === "0") return "simple";
  } catch {
    /* no storage */
  }
  return DEFAULT[surface];
}

export function setAgentView(surface: AgentSurface, view: AgentView): void {
  try {
    localStorage.setItem(KEY[surface], view);
  } catch {
    /* not persisted */
  }
  try {
    window.dispatchEvent(new CustomEvent(AGENT_VIEW_EVENT, { detail: { surface, view } }));
  } catch {
    /* not in a window */
  }
}

export function toggleAgentView(surface: AgentSurface): AgentView {
  const next: AgentView = getAgentView(surface) === "detailed" ? "simple" : "detailed";
  setAgentView(surface, next);
  return next;
}
