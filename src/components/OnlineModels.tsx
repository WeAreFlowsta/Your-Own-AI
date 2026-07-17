/**
 * Online Models page.
 *
 * Frontier models (e.g. Claude) reached through Flowsta's privacy-preserving
 * passthrough proxy. Local features never need any of this — online models are
 * the one Pro feature gated behind a Flowsta sign-in (via Vault) + a plan.
 *
 * States: no Vault → get Vault; Vault locked → unlock; signed out → sign in
 * (the button + how-to instructions); signed in but no plan → link a plan;
 * fully enabled → the model catalog with pause/play (shared with offline models
 * via utils/modelPrefs, so paused models drop out of the AI picker).
 */

import { component$, useStore, useVisibleTask$, $ } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { getButtonUrl } from '@flowsta/login-button';
import {
  LuCloud,
  LuLock,
  LuPauseCircle,
  LuPlayCircle,
  LuShieldCheck,
} from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';
import { Callout } from './Callout';
import { getPausedModels, setModelPaused } from '../utils/modelPrefs';

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

interface OnlinePricing {
  input_per_mtok: number;
  output_per_mtok: number;
  request_fee_usd: number;
  search_per_call_usd?: number; // web-search models only
}

interface OnlineModel {
  id: string; // "online:<model>"
  display_name: string;
  description: string;
  context_window?: number;
  category?: string; // primary shelf — "chat" | "web_search" | "coding"
  categories?: string[]; // every shelf this model belongs to (newer catalogs)
  pricing?: OnlinePricing; // USD, margin applied — what the user pays
}

const MODEL_GROUPS = [
  { key: 'chat', label: 'Chat' },
  { key: 'web_search', label: 'Web search' },
  { key: 'coding', label: 'Coding' },
] as const;

// A model can belong to several shelves (a flagship that's also a top
// coder). Unknown or missing categories fold into Chat so a new catalog
// value never hides a model.
function modelGroupKeys(m: OnlineModel): string[] {
  const raw = m.categories?.length ? m.categories : [m.category];
  const keys = raw.map((c) => (MODEL_GROUPS.some((g) => g.key === c) ? c! : 'chat'));
  return [...new Set(keys)];
}

// The Auto router's recommended per-slot defaults (must mirror router.rs
// DEFAULT_FRESH / DEFAULT_HARD_CODE / DEFAULT_HARD_GENERAL - keep in sync).
// Surfaced as a badge so the page answers "which of these does Auto already
// use for me?" without the user opening Settings.
const AUTO_DEFAULTS: Record<string, string> = {
  'online:grok-4.5-search': 'Auto pick · fresh info',
  'online:gpt-5.6-sol': 'Auto pick · hard coding',
  'online:gpt-5.6-terra': 'Auto pick · hard questions',
};

const VAULT_DOWNLOAD_URL = 'https://flowsta.com/vault';

function formatContext(n?: number): string | null {
  if (!n || n <= 0) return null;
  return n >= 1000 ? `${Math.round(n / 1000)}K context` : `${n} context`;
}

function fmtUsd(n: number): string {
  return `$${n.toFixed(2)}`;
}

// Per-search fees are sub-cent — keep precision so they don't round to $0.01.
function fmtUsdSmall(n: number): string {
  if (n >= 0.01) return `$${n.toFixed(2)}`;
  return `$${n.toFixed(4).replace(/0+$/, '').replace(/\.$/, '')}`;
}

export const OnlineModels = component$(() => {
  const store = useStore({
    vault: null as VaultStatus | null,
    session: null as FlowstaSession | null,
    models: [] as OnlineModel[],
    paused: [] as string[],
    /** Catalog filter tab - same pattern as the Offline Models task tabs. */
    selectedGroup: 'all' as 'all' | 'chat' | 'web_search' | 'coding',
    busy: false,
    error: '',
    modelsError: false,
  });

  const refresh = $(async () => {
    try {
      const [v, s] = await Promise.all([
        invoke<VaultStatus>('flowsta_vault_status'),
        invoke<FlowstaSession>('flowsta_session'),
      ]);
      store.vault = v;
      store.session = s;
    } catch (e) {
      console.warn('[OnlineModels] status check failed:', e);
    }
  });

  const loadModels = $(async () => {
    try {
      store.models = await invoke<OnlineModel[]>('list_online_models');
      store.modelsError = false;
    } catch {
      store.modelsError = true;
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    refresh();
    loadModels();
    store.paused = [...getPausedModels()];
    // Live-poll Vault/session while signed out so installing/unlocking Vault or
    // approving sign-in updates this page without a restart. Mirrors FlowstaAccount.
    const interval = setInterval(() => {
      if (!store.session?.signed_in && !store.busy) refresh();
    }, 5000);
    cleanup(() => clearInterval(interval));
  });

  const handleSignIn = $(async () => {
    store.busy = true;
    store.error = '';
    try {
      store.session = await invoke<FlowstaSession>('flowsta_sign_in');
    } catch (e) {
      const msg = String(e);
      if (msg.includes('vault_locked')) {
        store.error = 'Your Vault is locked — unlock it and try again.';
      } else if (msg.includes('vault_not_found')) {
        store.error = "Flowsta Vault isn't running — start it and try again.";
      } else if (msg.includes('vault_interrupted')) {
        store.error =
          'Vault stopped responding before sign-in finished (it may have locked). Unlock Vault and try again.';
      } else if (msg.includes('denied') || msg.includes('rejected') || msg.includes('user_denied')) {
        store.error = 'Sign-in was declined in Vault.';
      } else {
        store.error = `Sign-in failed: ${msg}`;
      }
      await refresh();
    } finally {
      store.busy = false;
      await refresh();
    }
  });

  // "Choose a plan" / link a not-yet-linked device → carries ?link=<key>.
  const handleLinkPlan = $(async () => {
    try {
      const url = await invoke<string>('flowsta_link_url');
      await openUrl(url);
    } catch (e) {
      store.error = String(e);
    }
  });

  // "Manage plan" → plain account page (NO ?link=); the device is already set
  // up, so don't re-trigger the account page's link-confirmation prompt.
  const handleManagePlan = $(async () => {
    try {
      const url = await invoke<string>('flowsta_account_url');
      await openUrl(url);
    } catch (e) {
      store.error = String(e);
    }
  });

  const handleSignOut = $(async () => {
    await invoke('flowsta_sign_out');
    await refresh();
  });

  const handleTogglePause = $((id: string) => {
    const paused = !store.paused.includes(id);
    setModelPaused(id, paused);
    store.paused = paused
      ? [...store.paused, id]
      : store.paused.filter((n) => n !== id);
  });

  const signedIn = !!store.session?.signed_in;
  const notLinked = signedIn && store.session?.linked === false;
  const usable = signedIn && !notLinked; // online models actually work

  return (
    <div class="max-w-7xl mx-auto px-6">
      {/* Header */}
      <div class="mb-6">
        <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-1">
          Online Models
        </h2>
        <p class="text-sm text-[var(--text-secondary)] mb-4">
          Powerful cloud models you reach through Flowsta — for tasks beyond what
          your own hardware can run.
        </p>

        <div class="space-y-3">
          <Callout intent="premium" title="What you get" id="online-what-you-get">
            <ul class="list-disc space-y-1.5 pl-5 marker:text-[var(--text-muted)]">
              <li>More capable than the models your own computer can run — for harder or bigger tasks.</li>
              <li>Some can search the web for current, up-to-date answers — look for "Web" in the model name.</li>
            </ul>
          </Callout>

          <Callout intent="info" title="Your privacy" id="online-privacy">
            <ul class="list-disc space-y-1.5 pl-5 marker:text-[var(--text-muted)]">
              <li>When Flowsta forwards your message to a provider, it doesn't attach your name, account, or any personal details.</li>
              <li>
                But the message itself reaches that provider, and we can't control what
                they do with it. If you'd rather no one outside ever sees a query, use an{' '}
                <strong class="text-[var(--text-primary)]">Offline model</strong> — it
                never leaves your device.
              </li>
              <li>
                Either way, your chat history — online and offline — is{' '}
                <strong class="text-[var(--text-primary)]">encrypted inside Your Own AI</strong>.
                No one can read it unless you grant access.
              </li>
            </ul>
          </Callout>
        </div>
      </div>

      {/* Error */}
      {store.error && (
        <div class="mb-6 rounded-lg border border-red-800 bg-red-900/30 p-3 text-sm text-red-300">
          {store.error}
        </div>
      )}

      {/* ── Gating panel ─────────────────────────────────────────────────── */}
      {!store.vault ? (
        <div class="generic-container rounded-2xl p-6 mb-8">
          <p class="text-sm text-[var(--text-muted)]">Checking for Flowsta Vault…</p>
        </div>
      ) : !store.vault.installed ? (
        <div class="generic-container rounded-2xl p-6 mb-8 space-y-3">
          <h3 class="text-lg font-semibold text-[var(--text-primary)]">
            Step 1 — Get Flowsta Vault
          </h3>
          <p class="text-sm text-[var(--text-secondary)]">
            Vault holds your Flowsta identity on your own device — no passwords on
            servers — and is how Your Own AI signs you in. Install it, then come
            back here.
          </p>
          <div>
            <LiquidMetalButton onClick$={() => openUrl(VAULT_DOWNLOAD_URL)}>
              <span class="px-5 py-2.5 text-sm">Get Flowsta Vault</span>
            </LiquidMetalButton>
            <button
              class="ml-3 text-sm text-[var(--text-link)] hover:underline"
              onClick$={refresh}
            >
              I've installed it — check again
            </button>
          </div>
        </div>
      ) : !signedIn ? (
        <div class="generic-container rounded-2xl p-6 mb-8">
          <div class="flex items-start gap-3 mb-4">
            <LuShieldCheck class="w-6 h-6 text-[var(--text-link)] shrink-0 mt-0.5" />
            <div>
              <h3 class="text-lg font-semibold text-[var(--text-primary)] mb-1">
                Sign in to turn on online models
              </h3>
              <p class="text-sm text-[var(--text-secondary)]">
                Online models are a Pro feature. Here's how to enable them:
              </p>
            </div>
          </div>

          <ol class="space-y-2 mb-5 text-sm text-[var(--text-secondary)]">
            <li class="flex gap-2">
              <span class="text-[var(--text-muted)]">1.</span>
              <span>
                <span class="text-[var(--text-primary)] font-medium">Install &amp; unlock Flowsta Vault</span>
                {' '}— it holds your Flowsta identity on this device.
                {store.vault.installed && (
                  <span class="text-emerald-400"> ✓ installed</span>
                )}
              </span>
            </li>
            <li class="flex gap-2">
              <span class="text-[var(--text-muted)]">2.</span>
              <span>
                <span class="text-[var(--text-primary)] font-medium">Sign in with Flowsta</span>
                {' '}— Vault asks you to approve; nothing happens without your say-so.
              </span>
            </li>
            <li class="flex gap-2">
              <span class="text-[var(--text-muted)]">3.</span>
              <span>
                <span class="text-[var(--text-primary)] font-medium">Choose a plan</span>
                {' '}at yourownai.net — usage is metered, no lock-in.
              </span>
            </li>
          </ol>

          {!store.vault.unlocked && (
            <p class="text-sm text-amber-300 mb-3">
              Vault is locked — unlock it, then sign in.
            </p>
          )}

          <button
            type="button"
            onClick$={handleSignIn}
            disabled={store.busy || !store.vault.unlocked}
            aria-label="Sign in with Flowsta"
            class="block cursor-pointer transition-opacity hover:opacity-85 disabled:cursor-not-allowed disabled:opacity-50"
            style={{ border: 'none', background: 'transparent', padding: '0' }}
          >
            <img
              src={getButtonUrl('dark', 'pill')}
              alt="Sign in with Flowsta"
              width={175}
              height={40}
            />
          </button>
          <p class="text-xs text-[var(--text-muted)] mt-2">
            {store.busy
              ? 'Waiting for Vault approval…'
              : 'Vault will ask you to approve the sign-in.'}
          </p>
        </div>
      ) : notLinked ? (
        <div class="generic-container rounded-2xl p-6 mb-8 space-y-3">
          <h3 class="text-lg font-semibold text-[var(--text-primary)]">
            Last step — choose a plan
          </h3>
          <p class="text-sm text-[var(--text-secondary)]">
            You're signed in. To use online models on this device, link it to your
            yourownai.net plan (or subscribe) in the browser — it takes a few
            seconds. Usage is metered, no lock-in.
          </p>
          <div>
            <LiquidMetalButton onClick$={handleLinkPlan}>
              <span class="px-5 py-2 text-sm">Choose a plan</span>
            </LiquidMetalButton>
            <button
              class="ml-3 text-sm text-[var(--text-link)] hover:underline"
              onClick$={refresh}
            >
              I've linked it — refresh
            </button>
          </div>
        </div>
      ) : (
        <div class="generic-container rounded-2xl p-4 mb-8 flex items-center justify-between gap-3">
          <div class="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
            <LuShieldCheck class="w-5 h-5 text-emerald-400 shrink-0" />
            <span>
              Online models are on
              {store.session?.display_name && (
                <span class="text-[var(--text-muted)]"> · {store.session.display_name}</span>
              )}
              {store.session?.tier && store.session.tier !== 'free' && (
                <span class="ml-2 rounded-full bg-emerald-900/50 px-2 py-0.5 text-xs font-medium text-emerald-300">
                  {store.session.tier.toUpperCase()}
                </span>
              )}
            </span>
          </div>
          <div class="flex items-center gap-4 shrink-0">
            <button class="text-xs text-[var(--text-link)] hover:underline" onClick$={handleManagePlan}>
              Manage plan
            </button>
            <button class="text-xs text-[var(--text-muted)] hover:underline" onClick$={handleSignOut}>
              Sign out
            </button>
          </div>
        </div>
      )}

      {/* ── Models catalog ───────────────────────────────────────────────── */}
      <div class="mb-8">
        <h3 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-1 border-b border-[var(--border-subtle)] pb-2">
          Available Models
        </h3>
        <p class="text-xs text-[var(--text-muted)] mb-4">
          Pay-as-you-go — prices are per million tokens (≈ 750,000 words) and count
          against your plan's monthly allowance.
        </p>

        {store.modelsError ? (
          <p class="text-center py-8 text-sm text-[var(--text-muted)]">
            Couldn't reach the online model catalog — check your connection and try again.
          </p>
        ) : store.models.length === 0 ? (
          <p class="text-center py-8 text-sm text-[var(--text-muted)]">
            No online models available right now.
          </p>
        ) : (
          <>
            {/* Filter tabs - one grid underneath, a model shows on every tab
                it belongs to (the categories chips on the card say which). */}
            <div class="flex gap-1 overflow-x-auto pb-1 mb-5">
              {[{ key: 'all' as const, label: 'All' }, ...MODEL_GROUPS].map((tab) => {
                const isActive = store.selectedGroup === tab.key;
                const count = tab.key === 'all'
                  ? store.models.length
                  : store.models.filter((m) => modelGroupKeys(m).includes(tab.key)).length;
                return (
                  <button
                    key={tab.key}
                    type="button"
                    onClick$={() => { store.selectedGroup = tab.key; }}
                    class={`px-4 py-2 rounded-full text-sm font-medium whitespace-nowrap transition-all ${
                      isActive
                        ? 'bg-[var(--bg-button-primary)] text-[var(--text-button-primary)] shadow-md'
                        : 'bg-[var(--bg-card)] text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)]'
                    }`}
                  >
                    {tab.label}
                    <span class={`ml-1.5 text-xs ${isActive ? 'opacity-80' : 'opacity-50'}`}>
                      {count}
                    </span>
                  </button>
                );
              })}
            </div>
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
            {store.models
              .filter((m) => store.selectedGroup === 'all' || modelGroupKeys(m).includes(store.selectedGroup))
              .map((model) => {
              const isPaused = store.paused.includes(model.id);
              const ctx = formatContext(model.context_window);
              return (
                <div
                  key={model.id}
                  class={`generic-container rounded-2xl overflow-hidden flex flex-col justify-between transition-all hover:shadow-2xl transform hover:-translate-y-1 ${
                    (usable && isPaused) || !usable ? 'opacity-75' : ''
                  }`}
                >
                  <div class="p-5">
                    <div class="flex items-center gap-2 mb-2">
                      <LuCloud class="w-4 h-4 text-[var(--text-muted)] shrink-0" />
                      <h4 class="text-lg font-semibold text-[var(--text-primary)] leading-tight">
                        {model.display_name}
                      </h4>
                      <div class="flex flex-wrap gap-1 ml-auto">
                        {usable && isPaused && (
                          <span class="px-2 py-0.5 bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] text-[var(--text-muted)] text-[10px] rounded-full font-semibold whitespace-nowrap">
                            Paused
                          </span>
                        )}
                        {modelGroupKeys(model).map((k) => (
                          <span
                            key={k}
                            class="px-2 py-0.5 bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] text-[var(--text-muted)] text-[10px] rounded-full font-medium whitespace-nowrap"
                          >
                            {MODEL_GROUPS.find((g) => g.key === k)?.label}
                          </span>
                        ))}
                      </div>
                    </div>

                    <p class="text-sm text-[var(--text-secondary)] mb-3 line-clamp-3">
                      {model.description}
                    </p>

                    <div class="space-y-2">
                      {AUTO_DEFAULTS[model.id] && (
                        <span
                          class="inline-block px-2 py-0.5 mr-1.5 rounded-full bg-emerald-900/50 text-emerald-300 text-[10px] font-semibold whitespace-nowrap"
                          title="When an AI is set to Auto, this is the model the router picks for this kind of question (changeable in Settings - Routing)"
                        >
                          {AUTO_DEFAULTS[model.id]}
                        </span>
                      )}
                      {ctx && (
                        <span class="inline-block px-2 py-0.5 bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] rounded text-xs text-[var(--text-primary)]">
                          {ctx}
                        </span>
                      )}
                      {model.pricing && (
                        <div class="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-[var(--text-secondary)]">
                          <span class="text-[var(--text-muted)]">Cost</span>
                          <span>
                            <span class="font-medium text-[var(--text-primary)]">
                              {fmtUsd(model.pricing.input_per_mtok)}
                            </span>{' '}/ 1M in
                          </span>
                          <span>
                            <span class="font-medium text-[var(--text-primary)]">
                              {fmtUsd(model.pricing.output_per_mtok)}
                            </span>{' '}/ 1M out
                          </span>
                          {model.pricing.request_fee_usd > 0 && (
                            <span>+ {fmtUsd(model.pricing.request_fee_usd)} / request</span>
                          )}
                          {!!model.pricing.search_per_call_usd && model.pricing.search_per_call_usd > 0 && (
                            <span>+ {fmtUsdSmall(model.pricing.search_per_call_usd)} / web search</span>
                          )}
                        </div>
                      )}
                    </div>
                  </div>

                  <div class="px-5 py-3 mt-auto">
                    {usable ? (
                      <div class="flex justify-end">
                        <LiquidMetalButton
                          variant="secondary"
                          onClick$={() => handleTogglePause(model.id)}
                          title={isPaused ? 'Resume — show this model in the AI picker' : 'Pause — hide this model from the AI picker'}
                          class="p-2 transition-colors"
                        >
                          {isPaused ? (
                            <LuPlayCircle class="w-[18px] h-[18px]" />
                          ) : (
                            <LuPauseCircle class="w-[18px] h-[18px]" />
                          )}
                        </LiquidMetalButton>
                      </div>
                    ) : (
                      <div class="flex items-center justify-center gap-1.5 text-xs text-[var(--text-muted)]">
                        <LuLock class="w-3.5 h-3.5" />
                        {signedIn ? 'Choose a plan to use' : 'Sign in to use'}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
            </div>
          </>
        )}
      </div>
    </div>
  );
});

export default OnlineModels;
