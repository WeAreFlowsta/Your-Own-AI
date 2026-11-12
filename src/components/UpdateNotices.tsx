/**
 * The app's own updates, bottom right: an update downloaded and checked,
 * waiting for "Restart to update"; each step of the install once pressed;
 * and once after an update, what this copy was updated to.
 */
import { component$, useSignal, useVisibleTask$ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { carryOverOldUpdateChoice, updateStatus, type ReadyUpdate, type UpdatedFrom } from "../utils/updates";
import LiquidMetalButton from "./LiquidMetalButton";

type Stage = "backup" | "backup-running" | "password" | "installing" | "restarting" | "failed" | "nobackup";

export const UpdateNotices = component$(() => {
  const ready = useSignal<ReadyUpdate | null>(null);
  const stage = useSignal<Stage | null>(null);
  // Why no backup could be made before the install ("vault_locked" or
  // "vault_unavailable"), while the person decides (stage "nobackup").
  const noBackupWhy = useSignal<string | null>(null);
  const updated = useSignal<UpdatedFrom | null>(null);
  const notice = useSignal<string | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await carryOverOldUpdateChoice();
    try {
      const st = await updateStatus();
      ready.value = st.ready;
      updated.value = st.updated;
      notice.value = st.notice;
    } catch {
      /* the updater is not in this build */
    }
    const offReady = await listen<ReadyUpdate>("update-ready", (e) => {
      ready.value = e.payload;
    });
    const offStage = await listen<Exclude<Stage, "failed">>("update-stage", (e) => {
      stage.value = e.payload;
    });
    cleanup(() => {
      offReady();
      offStage();
    });
  });

  if (!ready.value && !updated.value && !notice.value) return null;
  return (
    <div class="fixed bottom-4 right-4 z-[60] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2" data-testid="update-notices">
      {updated.value && (
        <div data-testid="update-done" class="flex items-start gap-3 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] px-4 py-3 shadow-xl">
          <div class="min-w-0 flex-1">
            <p class="text-sm text-[var(--text-primary)]">Updated to Your Own AI {updated.value.to}</p>
            {updated.value.notes_url && (
              <button
                type="button"
                class="mt-0.5 text-xs text-[var(--text-link)] hover:underline"
                onClick$={async () => {
                  const { openUrl } = await import("@tauri-apps/plugin-opener");
                  await openUrl(updated.value!.notes_url);
                }}
              >
                What's new
              </button>
            )}
          </div>
          <button
            type="button"
            aria-label="Dismiss"
            class="shrink-0 text-[var(--text-muted)] hover:text-[var(--text-primary)]"
            onClick$={async () => {
              updated.value = null;
              await invoke("dismiss_updated").catch(() => {});
            }}
          >
            ✕
          </button>
        </div>
      )}

      {notice.value && !ready.value && (
        <div data-testid="update-notice" class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] px-4 py-3 text-sm text-[var(--text-secondary)] shadow-xl">
          {notice.value}
        </div>
      )}

      {ready.value && (
        <div data-testid="update-ready" class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] px-4 py-3 shadow-xl">
          {stage.value === "nobackup" ? (
            <>
              <p class="text-sm text-[var(--text-primary)]">Your Own AI {ready.value.version} is ready to install</p>
              <p class="mt-0.5 text-xs text-[var(--text-secondary)]" data-testid="update-no-backup">
                {noBackupWhy.value === "vault_unavailable"
                  ? "Flowsta Vault is not running, so this update cannot back up to it first."
                  : "Flowsta Vault is locked, so this update cannot back up to it first."}
              </p>
              <div class="mt-2 flex items-center justify-end gap-2">
                <LiquidMetalButton
                  variant="secondary"
                  class="px-3 py-1.5 text-xs"
                  testId="update-without-backup"
                  onClick$={async () => {
                    stage.value = ready.value!.asks_password ? "password" : "installing";
                    try {
                      await invoke("restart_to_update", { withoutBackup: true });
                    } catch (e) {
                      console.error("Update did not install:", e);
                      stage.value = "failed";
                    }
                  }}
                >
                  Update without a backup
                </LiquidMetalButton>
                <LiquidMetalButton
                  class="px-3 py-1.5 text-xs"
                  testId="update-unlock-first"
                  title="Unlock Flowsta Vault, then choose Restart to update again."
                  onClick$={() => {
                    stage.value = null;
                  }}
                >
                  {noBackupWhy.value === "vault_unavailable" ? "Open the Vault first" : "Unlock the Vault first"}
                </LiquidMetalButton>
              </div>
            </>
          ) : stage.value === null || stage.value === "failed" ? (
            <>
              <p class="text-sm text-[var(--text-primary)]">Your Own AI {ready.value.version} is ready to install</p>
              {stage.value === "failed" && <p class="mt-0.5 text-xs text-[var(--text-secondary)]">It was not installed, and nothing changed.</p>}
              <div class="mt-2 flex items-center justify-end gap-2">
                {ready.value.notes_url && (
                  <LiquidMetalButton
                    variant="secondary"
                    class="px-3 py-1.5 text-xs"
                    onClick$={async () => {
                      const { openUrl } = await import("@tauri-apps/plugin-opener");
                      await openUrl(ready.value!.notes_url);
                    }}
                  >
                    What's new
                  </LiquidMetalButton>
                )}
                <LiquidMetalButton
                  class="px-3 py-1.5 text-xs"
                  testId="restart-to-update"
                  title={
                    ready.value.asks_password
                      ? `Installs Your Own AI ${ready.value.version}, then closes and opens it again. Your computer asks for your password first.`
                      : `Installs Your Own AI ${ready.value.version}, then closes and opens it again.`
                  }
                  onClick$={async () => {
                    stage.value = ready.value!.asks_password ? "password" : "installing";
                    try {
                      await invoke("restart_to_update");
                    } catch (e) {
                      // No backup could be made: the person decides, the
                      // update never goes on without one in silence.
                      const why = /no_backup:(\w+)/.exec(String(e))?.[1];
                      if (why) {
                        noBackupWhy.value = why;
                        stage.value = "nobackup";
                        return;
                      }
                      console.error("Update did not install:", e);
                      stage.value = "failed";
                    }
                  }}
                >
                  Restart to update
                </LiquidMetalButton>
              </div>
            </>
          ) : (
            <div class="flex items-start gap-3">
              <span class="mt-1 h-3 w-3 shrink-0 animate-spin rounded-full border-2 border-[var(--text-secondary)] border-t-transparent" />
              <div class="min-w-0">
                <p class="text-sm text-[var(--text-primary)]">
                  {stage.value === "restarting" ? `Opening Your Own AI ${ready.value.version}` : `Installing Your Own AI ${ready.value.version}`}
                </p>
                <p class="mt-0.5 text-xs text-[var(--text-secondary)]">
                  {stage.value === "backup"
                    ? "Backing up to your Vault first."
                    : stage.value === "backup-running"
                    ? "Finishing a backup to your Vault that was already running."
                    : stage.value === "password"
                    ? "Your computer asks for your password, then Your Own AI closes and opens again."
                    : stage.value === "installing"
                      ? "Your Own AI closes and opens again in a moment."
                      : "Starting the new version."}
                </p>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
});

export default UpdateNotices;
