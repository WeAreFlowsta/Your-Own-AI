/**
 * Whether an AI's replies can be made off this computer, from its model
 * setting: an online model, an automatic choice that may pick one, or a
 * server the person runs elsewhere. Words only, no imports.
 */
export function canAnswerOnline(model: string | undefined | null): boolean {
  const m = (model ?? "").trim();
  return m.startsWith("online:") || m.startsWith("external:") || m === "auto:online-offline" || m === "auto:online";
}
