import { component$, type QRL, type Signal } from "@builder.io/qwik";
import { LuShieldCheck, LuShieldAlert, LuShield } from "@qwikest/icons/lucide";
import LiquidMetalButton from "./LiquidMetalButton";
import type { AiPack } from "../utils/aiPack";
import type { VerifyState } from "../utils/packSigning";

interface Props {
  pack: Signal<AiPack | null>;
  verify: Signal<VerifyState | null>;
  importing: Signal<boolean>;
  archetypeLabel: string;
  onConfirm$: QRL<() => void>;
  onCancel$: QRL<() => void>;
}

/**
 * Import preview - who this AI is before it joins your list, with the
 * signature verdict up front. Tampered packs cannot be imported.
 */
export default component$<Props>(
  ({ pack, verify, importing, archetypeLabel, onConfirm$, onCancel$ }) => {
    const p = pack.value;
    if (!p) return null;
    const v = verify.value ?? "unsigned";

    return (
      <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
        <div class="bg-[var(--bg-header-footer)] w-full max-w-md rounded-xl p-6 shadow-2xl">
          <h3 class="text-lg font-semibold text-[var(--text-primary)]">Import AI</h3>

          <div class="mt-4 flex items-center gap-4">
            {p.thumbnail ? (
              <img
                src={p.thumbnail}
                alt=""
                width={64}
                height={64}
                class="h-16 w-16 rounded-full object-cover border border-[var(--border-subtle)]"
              />
            ) : (
              <div class="flex h-16 w-16 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-[var(--bg-card)] text-2xl">
                {p.emoji || "🤖"}
              </div>
            )}
            <div class="min-w-0">
              <p class="font-semibold text-[var(--text-primary)]">{p.name}</p>
              <p class="text-xs text-[var(--text-secondary)]">
                {archetypeLabel}
                {p.knowledge.length > 0 &&
                  ` · ${p.knowledge.length} knowledge item${p.knowledge.length === 1 ? "" : "s"}`}
              </p>
            </div>
          </div>

          {p.description && (
            <p class="mt-3 text-sm text-[var(--text-secondary)] line-clamp-3">
              {p.description}
            </p>
          )}

          <div
            class={`mt-4 flex items-center gap-2 rounded-lg border px-3 py-2 text-xs ${
              v === "verified"
                ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-500"
                : v === "tampered"
                  ? "border-red-500/30 bg-red-500/10 text-red-500"
                  : "border-[var(--border-subtle)] bg-[var(--bg-card)] text-[var(--text-secondary)]"
            }`}
          >
            {v === "verified" ? (
              <>
                <LuShieldCheck class="h-4 w-4 shrink-0" />
                Verified pack - signed by its maker's Flowsta identity.
              </>
            ) : v === "tampered" ? (
              <>
                <LuShieldAlert class="h-4 w-4 shrink-0" />
                This pack's contents don't match its signature. It may have
                been altered - it can't be imported.
              </>
            ) : (
              <>
                <LuShield class="h-4 w-4 shrink-0" />
                Unsigned pack - only import files from someone you trust.
              </>
            )}
          </div>

          <p class="mt-3 text-xs text-[var(--text-muted)]">
            Imports as a new AI on "Auto - Offline Only" - change its model
            anytime. No conversations or personal memories come with a pack.
          </p>

          <div class="mt-5 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end sm:gap-3">
            <LiquidMetalButton
              variant="secondary"
              onClick$={onCancel$}
              disabled={importing.value}
              class="px-5 py-2 text-sm"
            >
              Cancel
            </LiquidMetalButton>
            {v !== "tampered" && (
              <LiquidMetalButton
                onClick$={onConfirm$}
                disabled={importing.value}
                class="px-5 py-2 text-sm"
              >
                {importing.value ? "Importing..." : `Add ${p.name}`}
              </LiquidMetalButton>
            )}
          </div>
        </div>
      </div>
    );
  },
);
