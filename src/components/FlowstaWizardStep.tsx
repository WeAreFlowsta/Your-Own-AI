import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getButtonUrl } from "@flowsta/login-button";
import LiquidMetalButton from "./LiquidMetalButton";

interface VaultStatus { installed: boolean; unlocked: boolean; present?: boolean }
interface FlowstaSession { signed_in: boolean; display_name: string | null; web_username: string | null }

const VAULT_DOWNLOAD_URL = "https://flowsta.com/vault/?from=app&app=your-own-ai";

/**
 * The wizard's optional sign-in (VAULT_CONNECTION_UX.md fix 12): one line on
 * what the Vault does, then the one action the Vault's state calls for. It
 * never blocks the wizard - "Later" is the default, and Settings has the full
 * card.
 */
export default component$(() => {
  const vault = useSignal<VaultStatus | null>(null);
  const session = useSignal<FlowstaSession | null>(null);
  const busy = useSignal(false);
  const error = useSignal("");
  const openingVault = useSignal(false);

  const refresh = $(async () => {
    try {
      const [v, s] = await Promise.all([invoke<VaultStatus>("flowsta_vault_status"), invoke<FlowstaSession>("flowsta_session")]);
      vault.value = v;
      session.value = s;
    } catch { /* the card stays in its last state */ }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    void refresh();
    const t = setInterval(() => { if (!busy.value && !session.value?.signed_in) void refresh(); }, 5000);
    cleanup(() => clearInterval(t));
  });

  const signIn = $(async () => {
    busy.value = true;
    error.value = "";
    try {
      session.value = await invoke<FlowstaSession>("flowsta_sign_in");
    } catch (e) {
      const msg = String(e);
      error.value = msg.includes("vault_locked")
        ? "Your Vault is locked. Unlock it and try again."
        : msg.includes("vault_not_found")
          ? "Flowsta Vault isn't running. Start it and try again."
          : msg.includes("denied") || msg.includes("rejected")
            ? "You declined the sign-in in your Vault."
            : "Sign-in failed. Try again, or sign in later from Settings.";
      console.warn("[flowsta] wizard sign-in failed:", msg);
    } finally {
      busy.value = false;
      await refresh();
    }
  });

  if (session.value?.signed_in) {
    return (
      <div class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 mb-8 max-w-2xl" data-testid="wizard-flowsta">
        <p class="text-sm text-[var(--text-primary)]">
          Signed in with Flowsta{session.value.display_name ? ` as ${session.value.display_name}` : ""}. Your AIs and conversations back up to your Vault.
        </p>
      </div>
    );
  }

  return (
    <div class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 mb-8 max-w-2xl" data-testid="wizard-flowsta">
      <p class="text-sm font-medium text-[var(--text-primary)]">Optional: connect your Flowsta Vault</p>
      <p class="mt-1 text-xs text-[var(--text-secondary)]">
        Backs up your AIs and conversations, syncs them between devices, and signs you in to online models. Free, no password.
      </p>
      {error.value && <p class="mt-2 text-xs text-red-300">{error.value}</p>}
      <div class="mt-3 flex flex-wrap items-center gap-3">
        {!vault.value ? (
          <span class="text-xs text-[var(--text-muted)]">Checking for Flowsta Vault…</span>
        ) : vault.value.installed && vault.value.unlocked ? (
          <>
            <button
              type="button"
              onClick$={signIn}
              disabled={busy.value}
              aria-label="Sign in with Flowsta"
              class="block cursor-pointer transition-opacity hover:opacity-85 disabled:cursor-not-allowed disabled:opacity-50"
              style={{ border: "none", background: "transparent", padding: "0" }}
            >
              <img src={getButtonUrl("dark", "pill")} alt="Sign in with Flowsta" width={175} height={40} />
            </button>
            <span class="text-xs text-[var(--text-muted)]">{busy.value ? "Waiting for your Vault…" : "Your Vault asks you to approve."}</span>
          </>
        ) : vault.value.installed ? (
          <span class="text-xs text-[var(--text-muted)]">Your Vault is locked. Unlock it, then sign in here or later in Settings.</span>
        ) : vault.value.present ? (
          <>
            <LiquidMetalButton onClick$={async () => { openingVault.value = true; await openUrl("flowsta://open/v1"); setTimeout(() => { openingVault.value = false; }, 15000); }}>
              <span class="px-5 py-2 text-sm">Open Flowsta Vault</span>
            </LiquidMetalButton>
            {openingVault.value && <span class="text-xs text-[var(--text-muted)]">Opening Flowsta Vault…</span>}
          </>
        ) : (
          <LiquidMetalButton onClick$={() => openUrl(VAULT_DOWNLOAD_URL)}>
            <span class="px-5 py-2 text-sm">Get Flowsta Vault - free</span>
          </LiquidMetalButton>
        )}
        <span class="text-xs text-[var(--text-muted)]">Later is fine. It is in Settings under Your Flowsta Identity.</span>
      </div>
    </div>
  );
});
