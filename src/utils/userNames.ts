/**
 * The names a person goes by inside the app: their Flowsta display name and
 * username, and whatever they have told an AI their name is. Used for the
 * library's Mine guess (a document whose author field matches is theirs
 * until they say otherwise). Never sent anywhere.
 */
import { invoke } from "@tauri-apps/api/core";

export async function userNames(): Promise<string[]> {
  const names = new Set<string>();
  try {
    const s = await invoke<{ signed_in: boolean; display_name?: string | null; web_username?: string | null }>("flowsta_session");
    if (s?.display_name) names.add(s.display_name);
    if (s?.web_username) names.add(s.web_username);
  } catch {
    /* not signed in, or no session yet */
  }
  try {
    const { getFacts } = await import("./memory");
    for (const f of await getFacts()) {
      if (f.valid_to == null && f.predicate === "name_is" && f.value.trim()) names.add(f.value.trim());
    }
  } catch {
    /* records not ready */
  }
  return [...names];
}
