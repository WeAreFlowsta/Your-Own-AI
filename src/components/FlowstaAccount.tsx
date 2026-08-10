import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getButtonUrl } from "@flowsta/login-button";
import LiquidMetalButton from "./LiquidMetalButton";
import ConfirmModal from "./ConfirmModal";
import { useAiDataActions } from "../contexts/AiDataContext";
import {
  fetchUsage,
  formatUsage,
  setUsageTickerEnabled,
  usageTickerEnabled,
  type UsageSummary,
} from "../utils/usagePrefs";

interface VaultStatus {
  installed: boolean;
  unlocked: boolean;
}

interface FlowstaSession {
  signed_in: boolean;
  agent_pub_key: string | null;
  tier: string | null;
  linked: boolean | null;
  display_name: string | null;
  web_username: string | null;
  profile_picture: string | null;
}

interface EscrowStatus {
  state:
    | "synced"
    | "conflict"
    | "unlinked"
    | "vault_unavailable"
    | "vault_locked"
    | "identity_mismatch"
    | "error";
  local_conversations: number | null;
  error: string | null;
  backups_held: boolean;
}

interface RestoreStats {
  ais_added: number;
  ais_replaced: number;
  conversations_restored: number;
  records_restored: number;
  conversations_preserved: number;
  records_preserved: number;
  conversations_skipped: number;
  orphan_entries: number;
  missing_objects: number;
  missing_records: number;
  memory_facts_restored: boolean;
  thumbnails_restored: number;
  knowledge_restored: number;
}

const VAULT_DOWNLOAD_URL = "https://flowsta.com/vault";

/**
 * Flowsta account + backups settings. Vault detection, sign in with
 * Flowsta (via Vault's approval dialog), plan + Link-my-plan, and the
 * backups/recovery block. Local features never require any of it.
 *
 * `section` renders one half so the settings page can place the account
 * card first and "Backups & recovery" as its own section below: the two
 * halves share this component's state logic, so they stay one component
 * rendered twice rather than a copy-paste split.
 */
interface FlowstaAccountProps {
  section?: "account" | "backups" | "all";
}

export default component$<FlowstaAccountProps>((props) => {
  const section = props.section ?? "all";
  const vault = useSignal<VaultStatus | null>(null);
  const session = useSignal<FlowstaSession | null>(null);
  const busy = useSignal(false);
  const error = useSignal("");
  const escrow = useSignal<EscrowStatus | null>(null);
  // Outcome of the most recent backup attempt - a held or failed backup
  // must be visible here, never log-only (this section says backups are
  // automatic, so silence reads as success).
  const lastBackup = useSignal<{ status: string; reason?: string } | null>(null);
  const escrowBusy = useSignal(false);
  // Which confirmation dialog is open, if any.
  const confirmAction = useSignal<"restore" | "keep_local" | "restore_data" | null>(null);
  const restarting = useSignal(false);
  const restoreDataBusy = useSignal(false);
  const restoreDataResult = useSignal("");
  const restoreDataWarning = useSignal("");
  const { refreshUserAis } = useAiDataActions();

  const syncEscrow = $(async () => {
    if (escrowBusy.value) return;
    escrowBusy.value = true;
    try {
      escrow.value = await invoke<EscrowStatus>("vault_escrow_sync");
    } catch (e) {
      console.warn("[Flowsta] escrow sync failed:", e);
    } finally {
      escrowBusy.value = false;
    }
  });

  const usage = useSignal<UsageSummary | null>(null);
  const tickerOn = useSignal(false);

  const refresh = $(async () => {
    tickerOn.value = usageTickerEnabled();
    fetchUsage().then((u) => (usage.value = u));
    try {
      const [v, s] = await Promise.all([
        invoke<VaultStatus>("flowsta_vault_status"),
        invoke<FlowstaSession>("flowsta_session"),
      ]);
      vault.value = v;
      session.value = s;
      // Reconcile the transcript-key escrow whenever we're signed in. The
      // backend never overwrites a differing Vault copy - a mismatch just
      // reports "conflict" for the panel below.
      if (s.signed_in) await syncEscrow();
    } catch (e) {
      console.warn("[Flowsta] status check failed:", e);
    }
  });

  const handleRestore = $(async () => {
    const count = escrow.value?.local_conversations ?? 0;
    try {
      restarting.value = true;
      await invoke("vault_escrow_restore", { acceptDataLoss: count > 0 });
      // On success the app restarts; in dev builds it exits instead.
    } catch (e) {
      restarting.value = false;
      error.value = `Restore failed: ${String(e)}`;
    } finally {
      confirmAction.value = null;
    }
  });

  // Replay conversations (and any missing AIs) from the Vault "latest"
  // backup onto this device. Idempotent on the backend - conversations
  // already here are skipped, so re-running is always safe.
  const handleRestoreData = $(async () => {
    confirmAction.value = null;
    restoreDataBusy.value = true;
    restoreDataResult.value = "";
    restoreDataWarning.value = "";
    error.value = "";
    try {
      const stats = await invoke<RestoreStats>("vault_restore_conversations");
      const parts: string[] = [];
      if (stats.conversations_restored > 0) {
        parts.push(
          `Restored ${stats.conversations_restored} conversation${stats.conversations_restored === 1 ? "" : "s"} (${stats.records_restored} records).`
        );
      }
      const aisBack = stats.ais_added + stats.ais_replaced;
      if (aisBack > 0) {
        parts.push(`${aisBack} AI${aisBack === 1 ? "" : "s"} brought back.`);
      }
      if (stats.memory_facts_restored) {
        parts.push("Your Memory profile facts restored.");
      }
      if (stats.knowledge_restored > 0) {
        parts.push(
          `${stats.knowledge_restored} authored knowledge entr${stats.knowledge_restored === 1 ? "y" : "ies"} restored.`
        );
      }
      if (stats.conversations_preserved > 0) {
        parts.push(
          `${stats.conversations_preserved} conversation${stats.conversations_preserved === 1 ? "" : "s"} from deleted AIs preserved - kept in your data and backups, not shown in the app.`
        );
      }
      if (stats.conversations_restored === 0 && stats.conversations_preserved === 0) {
        parts.push(
          stats.conversations_skipped > 0
            ? "Everything in your Vault backup is already on this device."
            : "No conversations found in your Vault backup."
        );
      }
      restoreDataResult.value = parts.join(" ");
      // Partial-recovery honesty: anything the backup promised but could
      // not deliver is a warning, never silence.
      const warnings: string[] = [];
      if (stats.missing_objects > 0) {
        warnings.push(
          `${stats.missing_objects} conversation object${stats.missing_objects === 1 ? " was" : "s were"} missing or corrupted in the Vault (about ${stats.missing_records} record${stats.missing_records === 1 ? "" : "s"}) and could not be recovered. Everything else was restored.`
        );
      }
      if (stats.orphan_entries > 0) {
        warnings.push(
          `${stats.orphan_entries} record${stats.orphan_entries === 1 ? "" : "s"} in the backup had no parent conversation (truncated or partial backup) and could not be restored.`
        );
      }
      restoreDataWarning.value = warnings.join(" ");
      await refreshUserAis();
    } catch (e) {
      const msg = String(e);
      if (msg.includes("key_mismatch")) {
        error.value =
          "Your Vault backup was made under a different transcript key. Restore the key from Vault first (above), then try again.";
      } else if (msg.includes("no_backup")) {
        error.value = "No conversation backup found in your Vault yet.";
      } else if (msg.includes("identity_mismatch")) {
        error.value =
          "Your Vault is unlocked under a different identity than the one this device's data belongs to. Unlock the Vault that owns this data, or use \"Restore key from Vault\" to adopt the current identity.";
      } else if (msg.includes("vault_locked")) {
        error.value = "Your Vault is locked - unlock it and try again.";
      } else if (msg.includes("vault_unavailable")) {
        error.value = "Flowsta Vault isn't running - start it and try again.";
      } else {
        error.value = `Restore failed: ${msg}`;
      }
    } finally {
      restoreDataBusy.value = false;
    }
  });

  const handleKeepLocal = $(async () => {
    try {
      escrow.value = await invoke<EscrowStatus>("vault_escrow_keep_local");
    } catch (e) {
      error.value = `Could not update the Vault backup: ${String(e)}`;
    } finally {
      confirmAction.value = null;
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await refresh();
    try {
      lastBackup.value = await invoke<{ status: string; reason?: string } | null>(
        "last_backup_outcome"
      );
    } catch { /* older backend without the command */ }
    const unlistenBackup = await listen<{ status: string; reason?: string }>(
      "escrow-backup-outcome",
      (event) => (lastBackup.value = event.payload)
    );
    cleanup(() => unlistenBackup());
    // Live-poll Vault state until signed in — installing or unlocking
    // Vault updates this section without restarting the app.
    const interval = setInterval(async () => {
      // Don't poll while a sign-in is in flight — the authenticate call
      // holds a long request open on Vault, and concurrent /status polls
      // pile up against it. Also stop once signed in.
      if (!session.value?.signed_in && !busy.value) await refresh();
    }, 5000);
    cleanup(() => clearInterval(interval));
  });

  const handleSignIn = $(async () => {
    busy.value = true;
    error.value = "";
    try {
      session.value = await invoke<FlowstaSession>("flowsta_sign_in");
    } catch (e) {
      const msg = String(e);
      if (msg.includes("vault_locked")) {
        error.value = "Your Vault is locked — unlock it and try again.";
      } else if (msg.includes("vault_not_found")) {
        error.value = "Flowsta Vault isn't running — start it and try again.";
      } else if (msg.includes("vault_interrupted")) {
        error.value =
          "Vault stopped responding before sign-in finished (it may have locked). Unlock Vault and try again.";
      } else if (msg.includes("denied") || msg.includes("rejected") || msg.includes("user_denied")) {
        error.value = "Sign-in was declined in Vault.";
      } else {
        error.value = `Sign-in failed: ${msg}`;
      }
      // Re-probe so the section reflects Vault's real current state
      // (locked / gone) and offers the right next action.
      await refresh();
    } finally {
      busy.value = false;
      await refresh();
    }
  });

  const handleSignOut = $(async () => {
    await invoke("flowsta_sign_out");
    await refresh();
  });

  const handleLinkPlan = $(async () => {
    try {
      const url = await invoke<string>("flowsta_link_url");
      await openUrl(url);
    } catch (e) {
      error.value = String(e);
    }
  });

  const signedIn = () => !!session.value?.signed_in;

  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]">
      {section !== "backups" && (
        <>
      <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-1">
        Your Flowsta Account
      </h2>
      <p class="text-sm text-[var(--text-secondary)] mb-4">
        Everything local works without an account, forever. Connecting your
        Flowsta Vault — on this device, no passwords on servers — adds:
      </p>
      {!signedIn() && (
        <ul class="mb-4 space-y-1.5 text-sm text-[var(--text-secondary)]">
          <li class="flex items-start gap-2">
            <span class="text-emerald-400 mt-0.5">✓</span>
            <span>
              Your AI world backed up automatically — conversations, memory,
              and AI personalities — with one-click recovery if you ever lose
              this device.
            </span>
          </li>
          <li class="flex items-start gap-2">
            <span class="text-emerald-400 mt-0.5">✓</span>
            <span>
              Exports signed with your identity, so anyone can verify they
              really came from you.
            </span>
          </li>
          <li class="flex items-start gap-2">
            <span class="text-emerald-400 mt-0.5">✓</span>
            <span>Online frontier models, when you want them.</span>
          </li>
        </ul>
      )}

      {error.value && (
        <p class="mb-4 rounded-lg border border-red-800 bg-red-900/30 p-3 text-sm text-red-300">
          {error.value}
        </p>
      )}

      {!vault.value ? (
        <p class="text-sm text-[var(--text-muted)]">Checking for Flowsta Vault…</p>
      ) : !vault.value.installed ? (
        <div class="space-y-3">
          <p class="text-sm text-[var(--text-secondary)]">
            All of it comes with Flowsta Vault - the free app that holds your
            identity on your own device, no passwords on servers. Install it,
            unlock it, and this page signs you in automatically.
          </p>
          <LiquidMetalButton onClick$={() => openUrl(VAULT_DOWNLOAD_URL)}>
            <span class="px-5 py-2.5 text-sm">Get Flowsta Vault - free</span>
          </LiquidMetalButton>
          <button
            class="ml-3 text-sm text-[var(--text-link)] hover:underline"
            onClick$={refresh}
          >
            I've installed it — check again
          </button>
        </div>
      ) : !session.value?.signed_in ? (
        <div class="space-y-3">
          {!vault.value.unlocked && (
            <p class="text-sm text-amber-300">
              Vault is locked — unlock it, then sign in.
            </p>
          )}
          <button
            type="button"
            onClick$={handleSignIn}
            disabled={busy.value || !vault.value.unlocked}
            aria-label="Sign in with Flowsta"
            class="block cursor-pointer transition-opacity hover:opacity-85 disabled:cursor-not-allowed disabled:opacity-50"
            style={{ border: "none", background: "transparent", padding: "0" }}
          >
            <img
              src={getButtonUrl("dark", "pill")}
              alt="Sign in with Flowsta"
              width={175}
              height={40}
            />
          </button>
          <p class="text-xs text-[var(--text-muted)]">
            {busy.value
              ? "Waiting for Vault approval…"
              : "Vault will ask you to approve the sign-in."}
          </p>
        </div>
      ) : (
        <div class="space-y-4">
          <div class="flex items-center justify-between rounded-lg bg-[var(--bg-main)] p-4">
            <div class="flex min-w-0 items-center gap-3">
              {session.value.profile_picture ? (
                <img
                  src={session.value.profile_picture}
                  alt=""
                  width={40}
                  height={40}
                  class="h-10 w-10 shrink-0 rounded-full object-cover"
                />
              ) : (
                <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-[var(--bg-button-secondary)] font-varela text-white">
                  {(session.value.display_name || session.value.web_username || "?").charAt(0).toUpperCase()}
                </div>
              )}
              <div class="min-w-0">
                <p class="truncate text-sm text-[var(--text-primary)]">
                  {session.value.display_name || "Signed in via Vault"}
                  {session.value.web_username && (
                    <span class="ml-2 text-xs text-[var(--text-muted)]">@{session.value.web_username}</span>
                  )}
                </p>
                <p class="truncate font-mono text-xs text-[var(--text-muted)]">
                  {session.value.agent_pub_key?.slice(0, 12)}…{session.value.agent_pub_key?.slice(-6)}
                </p>
              </div>
            </div>
            <span
              class={`rounded-full px-3 py-1 text-xs font-medium ${
                session.value.tier && session.value.tier !== "free"
                  ? "bg-emerald-900/50 text-emerald-300"
                  : "bg-[var(--bg-input)] text-[var(--text-secondary)]"
              }`}
            >
              {(session.value.tier || "free").toUpperCase()}
            </span>
          </div>

          {usage.value && (
            <div class="rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-main)] p-4">
              <div class="flex items-center justify-between gap-3">
                <div>
                  <p class="text-sm text-[var(--text-primary)]">
                    {formatUsage(usage.value)} used this month
                  </p>
                  <p class="mt-0.5 text-xs text-[var(--text-muted)]">
                    {usage.value.requests} requests · allowance resets on the 1st
                    {usage.value.overage_opt_in && usage.value.overage_usd > 0
                      ? ` · $${usage.value.overage_usd.toFixed(2)} overage so far`
                      : ""}
                  </p>
                </div>
              </div>
              <div class="mt-3 flex items-center justify-between gap-3 border-t border-[var(--border-subtle)] pt-3">
                <div>
                  <p class="text-sm text-[var(--text-primary)]">Show usage in the header</p>
                  <p class="mt-0.5 text-xs text-[var(--text-muted)]">
                    A small live "{formatUsage(usage.value)}" pill beside the
                    model indicator. Off keeps the header clean.
                  </p>
                </div>
                <button
                  onClick$={() => {
                    setUsageTickerEnabled(!tickerOn.value);
                    tickerOn.value = !tickerOn.value;
                  }}
                  class={`relative h-6 w-11 shrink-0 rounded-full transition-colors ${
                    tickerOn.value ? "bg-emerald-500" : "bg-[var(--border-subtle)]"
                  }`}
                  aria-label="Show usage in the header"
                >
                  <span
                    class={`absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all ${
                      tickerOn.value ? "left-[22px]" : "left-0.5"
                    }`}
                  />
                </button>
              </div>
            </div>
          )}

          {session.value.linked === false && (
            <div class="rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-main)] p-4">
              <p class="text-sm font-medium text-[var(--text-primary)]">
                Unlock online frontier models
              </p>
              <p class="mt-1 text-xs text-[var(--text-muted)]">
                Chat with the best online models - GPT, Grok, Kimi, DeepSeek,
                Qwen, GLM, and Perplexity web search - on one simple plan.
                From $20/month, and every dollar comes back as model credit.
                Your local AIs stay free forever.
              </p>
              <div class="mt-3">
                <LiquidMetalButton onClick$={handleLinkPlan}>
                  <span class="px-5 py-2 text-sm">See plans</span>
                </LiquidMetalButton>
              </div>
              <p class="mt-2 text-xs text-[var(--text-muted)]">
                Already subscribed on yourownai.net? The same button links
                this device to your plan - or{" "}
                <button
                  class="text-[var(--text-link)] hover:underline"
                  onClick$={refresh}
                >
                  refresh
                </button>{" "}
                if you just did.
              </p>
            </div>
          )}

          {session.value.linked !== false &&
            session.value.tier &&
            session.value.tier !== "free" && (
              <button
                class="text-sm text-[var(--text-link)] hover:underline"
                onClick$={async () => {
                  const url = await invoke<string>("flowsta_account_url");
                  await openUrl(url);
                }}
              >
                Manage plan on yourownai.net
              </button>
            )}

          {escrow.value?.state === "identity_mismatch" && (
            <div class="rounded-lg border border-amber-700/60 bg-amber-900/20 p-4">
              <p class="text-sm font-medium text-amber-200">
                Your Vault holds a different identity
              </p>
              <p class="mt-1 text-xs text-[var(--text-secondary)]">
                The data on this device belongs to a different Flowsta identity
                than the one your Vault is unlocked with, so automatic backups
                are paused - they would overwrite the other identity's backup.
                Unlock the Vault that owns this data to resume, or use
                "Restore key from Vault" below to adopt the current identity
                (this replaces what's on this device).
              </p>
            </div>
          )}

          <button
            class="text-sm text-[var(--text-muted)] underline hover:text-[var(--text-secondary)]"
            onClick$={handleSignOut}
          >
            Sign out
          </button>
        </div>
      )}
        </>
      )}

      {/* Backups & recovery — the CAL story. The recovery KEY is escrowed
          automatically when signed in; the copy stays precise about which
          is which. Standalone section when `section === "backups"`. */}
      {section !== "account" && (
      <div class={section === "backups" ? "" : "mt-6 border-t border-[var(--border-subtle)] pt-4"}>
        {section === "backups" ? (
          <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-1">
            Backups & recovery
          </h2>
        ) : (
          <h3 class="text-sm font-semibold text-[var(--text-primary)]">
            Backups & data export
          </h3>
        )}
        <p class="mt-1 text-sm text-[var(--text-secondary)]">
          Your AIs' conversations live on this device, encrypted with a key
          only you hold. While you're signed in, Your Own AI automatically
          backs up that key AND your conversations to your Flowsta Vault -
          and Vault's "Download Export" hands you all of it, readable, with
          the keys, yours to take anywhere. No lock-in, by design.
        </p>
        {/* Key conflict = the start of the RECOVERY story, so it lives here
            with the rest of it, framed as the two steps it actually is.
            (It used to sit in the account card above, which read as a
            second, competing "restore from Vault" narrative.) */}
        {signedIn() && escrow.value?.state === "conflict" && (
          <div class="mt-3 rounded-lg border border-amber-700/60 bg-amber-900/20 p-4">
            <p class="text-sm font-medium text-amber-200">
              Bringing this device back from your Vault takes two steps
            </p>
            <p class="mt-1 text-xs text-[var(--text-secondary)]">
              Your Vault's backup was made under a different key than this
              device is using - usually because this is a fresh install
              while your Vault kept the key from the previous one.{" "}
              {(escrow.value.local_conversations ?? 0) > 0
                ? `Step 1 restores the Vault's key (this device has ${escrow.value.local_conversations} conversation record${escrow.value.local_conversations === 1 ? "" : "s"} under its own key - restoring deletes ${escrow.value.local_conversations === 1 ? "it" : "them"}, and the app restarts). `
                : "Step 1 restores the Vault's key (nothing has been written under this device's key yet, so this is safe - the app restarts). "}
              Step 2, after the restart: "Restore conversations from Vault"
              below brings everything back.
            </p>
            <div class="mt-3 flex flex-col-reverse gap-2 sm:flex-row sm:items-center">
              <button
                class="rounded-full border border-[var(--border-subtle)] px-5 py-2 text-sm text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:border-[var(--text-muted)]"
                onClick$={() => (confirmAction.value = "keep_local")}
              >
                Keep this device's key
              </button>
              <LiquidMetalButton onClick$={() => (confirmAction.value = "restore")}>
                <span class="px-5 py-2 text-sm">Step 1: Restore the Vault's key</span>
              </LiquidMetalButton>
            </div>
          </div>
        )}
        {!signedIn() && section === "backups" && (
          <p class="mt-2 text-xs text-[var(--text-muted)]">
            Connect your Flowsta Vault above to turn on automatic backup.
          </p>
        )}
        {signedIn() && escrow.value?.state === "synced" && !escrow.value.backups_held && (
          <p class="mt-2 text-xs text-emerald-400">
            ✓ Recovery key and conversations back up to your Vault
            automatically.
          </p>
        )}
        {signedIn() && escrow.value?.backups_held && (
          <p class="mt-2 text-xs text-amber-300">
            Automatic backups are paused: your Vault backup may hold
            conversations this device doesn't (after a key restore or a
            reset). Restore conversations from Vault below to resume - or
            they resume on their own if the Vault backup turns out to be
            empty.
          </p>
        )}
        {signedIn() && escrow.value?.state === "vault_locked" && (
          <p class="mt-2 text-xs text-amber-300">
            Your Vault is locked - backups resume when you unlock it.
          </p>
        )}
        {/* The last backup attempt was refused or failed. The Vault-side
            restore hold has no local marker, so without this line the
            refusal would be invisible. ONE amber voice at a time: whenever
            a richer escrow-state panel (key conflict, locked, identity
            mismatch, this device's restore-pending) is already telling the
            story, this line stays silent - those states pause backups by
            definition and their panels carry the action to take. */}
        {signedIn() &&
          lastBackup.value &&
          lastBackup.value.status !== "ok" &&
          !(lastBackup.value.reason ?? "").includes("restore_pending") &&
          !(lastBackup.value.reason ?? "").includes("escrow_conflict") &&
          escrow.value?.state !== "vault_locked" &&
          escrow.value?.state !== "conflict" &&
          escrow.value?.state !== "identity_mismatch" &&
          !escrow.value?.backups_held && (
            <p class="mt-2 text-xs text-amber-300">
              {/* Substring match: vault refusals arrive wrapped (e.g.
                  "backup object conv-… rejected: restore_choice_pending"). */}
              {(lastBackup.value.reason ?? "").includes("restore_choice_pending")
                ? "Automatic backups are paused: your Vault was just restored and is waiting for you to import your Vault export (or choose to start fresh) in the Vault. Backups resume once you decide."
                : (lastBackup.value.reason ?? "").includes("escrow_conflict")
                  ? "Automatic backups are paused: your Vault holds recovery material for a different key than this device's. Restore conversations from Vault below, or resolve the key conflict, to resume."
                  : (lastBackup.value.reason ?? "").includes("empty_would_overwrite")
                    ? "Automatic backups are paused: the Vault backup has conversations this device doesn't. Restore conversations from Vault below to resume."
                    : (lastBackup.value.reason ?? "").includes("probe_failed")
                      ? "The last backup couldn't check the Vault first, so it held off. It retries automatically."
                      : `The last backup attempt didn't complete (${lastBackup.value.reason ?? "unknown"}). It retries automatically; the log file has detail.`}
            </p>
          )}
        {signedIn() && escrow.value?.state === "identity_mismatch" && section === "backups" && (
          <p class="mt-2 text-xs text-amber-300">
            Backups are paused: your Vault is unlocked under a different
            identity than the one this device's data belongs to. See Your
            Flowsta Account above.
          </p>
        )}
        {signedIn() && (
          <div class="mt-3">
            <button
              class="rounded-full border border-[var(--border-subtle)] px-5 py-2 text-sm text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:border-[var(--text-muted)] disabled:cursor-not-allowed disabled:opacity-50"
              disabled={restoreDataBusy.value}
              onClick$={() => (confirmAction.value = "restore_data")}
            >
              {restoreDataBusy.value
                ? "Restoring conversations…"
                : "Restore conversations from Vault"}
            </button>
            <p class="mt-2 text-xs text-[var(--text-muted)]">
              New device, or something missing? This brings the conversations
              (and AIs) in your Vault backup onto this device. Nothing here is
              deleted or overwritten.
            </p>
            {restoreDataResult.value && (
              <p class="mt-2 text-xs text-emerald-400">{restoreDataResult.value}</p>
            )}
            {restoreDataWarning.value && (
              <p class="mt-2 rounded-lg border border-amber-700/60 bg-amber-900/20 p-3 text-xs text-amber-200">
                {restoreDataWarning.value}
              </p>
            )}
            {/* The account half renders errors too, but as a separate
                component instance when the settings page splits sections -
                without this block, restore failures were invisible here. */}
            {error.value && section === "backups" && (
              <p class="mt-2 rounded-lg border border-red-800 bg-red-900/30 p-3 text-sm text-red-300">
                {error.value}
              </p>
            )}
          </div>
        )}
      </div>
      )}

      <ConfirmModal
        isOpen={confirmAction.value === "restore"}
        title="Restore transcript key from Vault?"
        message={
          (escrow.value?.local_conversations ?? 0) > 0
            ? `Your Own AI will switch to the key stored in your Vault and restart. The ${escrow.value?.local_conversations} conversation record${escrow.value?.local_conversations === 1 ? "" : "s"} on this device will be permanently deleted - they belong to this device's current key. That key is saved to a local file first, and your AIs and downloaded models stay untouched.`
            : "Your Own AI will switch to the key stored in your Vault and restart. Your AIs and downloaded models stay untouched, and this device's current key is saved to a local file first."
        }
        confirmLabel={restarting.value ? "Restarting…" : "Restore & restart"}
        variant={(escrow.value?.local_conversations ?? 0) > 0 ? "danger" : "default"}
        busy={restarting.value}
        onConfirm$={handleRestore}
        onCancel$={() => (confirmAction.value = null)}
      />
      <ConfirmModal
        isOpen={confirmAction.value === "restore_data"}
        title="Restore conversations from Vault?"
        message="Your Own AI will bring the conversations stored in your Vault backup onto this device, and add any AIs from the backup that aren't here yet. Conversations already on this device are skipped - nothing is deleted or overwritten."
        confirmLabel="Restore conversations"
        onConfirm$={handleRestoreData}
        onCancel$={() => (confirmAction.value = null)}
      />
      <ConfirmModal
        isOpen={confirmAction.value === "keep_local"}
        title="Keep this device's key?"
        message="The key stored in your Vault will be replaced with this device's key. Any transcripts created under the old key stay unreadable without it, so a copy of the old key is saved to a local file on this device before the switch."
        confirmLabel="Replace Vault backup"
        variant="danger"
        onConfirm$={handleKeepLocal}
        onCancel$={() => (confirmAction.value = null)}
      />
    </section>
  );
});
