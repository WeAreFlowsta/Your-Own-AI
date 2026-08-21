import { invoke } from "@tauri-apps/api/core";

/** Hard ceiling on a model load as seen from the UI. The Rust side bounds
 *  its own wait (~60s) and now serializes loads, but the UI must never be
 *  able to sit on "loading" forever again (the 0.5.0-beta Windows wedge):
 *  if the call outlives this, it resolves as the same MODEL_LOAD_TIMEOUT
 *  error the existing handlers already name and recover from. */
export const LOAD_MODEL_UI_CEILING_MS = 150_000;

export async function loadModelBounded(args: {
  filename: string;
  withVision: boolean;
  reason: string;
}): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      invoke("load_model", args),
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("MODEL_LOAD_TIMEOUT")),
          LOAD_MODEL_UI_CEILING_MS,
        );
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}
