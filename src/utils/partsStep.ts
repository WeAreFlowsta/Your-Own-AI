/** What the parts job does for a person with no model on this computer.
 *  A person whose AIs all answer online has no model download to wait
 *  for: memory's search part is fetched by itself, unless the connection
 *  is metered, and then the offer on the Memory page asks first. With no
 *  online AI yet there is nothing to fetch for. Pure, for the build check
 *  (scripts/check-parts-step.mjs). */
export function onlineOnlyStep(s: { aiModels: string[]; metered: boolean }): "no-ais-yet" | "ask-first" | "fetch" {
  const online = s.aiModels.some((m) => m.startsWith("online:") || m === "auto:online" || m === "auto:online-offline");
  if (!online) return "no-ais-yet";
  return s.metered ? "ask-first" : "fetch";
}
