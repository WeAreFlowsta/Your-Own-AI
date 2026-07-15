import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { LuCheck, LuCopy, LuChevronDown, LuLink } from "@qwikest/icons/lucide";
import { getLocalCustomAis } from "../utils/localAiStorage";

/** The local OpenAI-compatible endpoint external apps point at. */
const ENDPOINT = "http://localhost:11435/v1";

/** Model id an external caller uses for an AI. Mirrors the Rust resolver's
 *  slug (lowercase, non-alphanumerics → hyphens) so it matches GET /v1/models. */
function modelId(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** Self-contained connection snippet for one AI: everything another app
 *  needs (base URL + model) plus a runnable example. */
function setupSnippet(model: string): string {
  return [
    `Base URL: ${ENDPOINT}`,
    `Model: ${model}`,
    "",
    "Example:",
    `curl ${ENDPOINT}/chat/completions \\`,
    `  -H "Content-Type: application/json" \\`,
    `  -d '{"model": "${model}", "messages": [{"role": "user", "content": "Hello"}]}'`,
  ].join("\n");
}

/**
 * Settings → External app access.
 *
 * A lightweight status surface so users can discover that their custom AIs are
 * available to other apps on this computer, copy the endpoint, see exactly
 * which model name to use for each AI, and how to connect.
 */
export default component$(() => {
  const copiedKey = useSignal("");
  const showHow = useSignal(false);
  const ais = useSignal<{ name: string; model: string }[]>([]);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const list = await getLocalCustomAis();
      ais.value = list.map((a) => ({ name: a.name, model: modelId(a.name) }));
    } catch {
      /* none / store not ready */
    }
  });

  const copy = $(async (text: string, key: string) => {
    try {
      await navigator.clipboard.writeText(text);
      copiedKey.value = key;
      setTimeout(() => (copiedKey.value = ""), 1500);
    } catch {
      /* clipboard unavailable — no-op */
    }
  });

  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]">
      <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-1 flex items-center gap-2">
        <LuLink class="w-5 h-5 text-[var(--text-secondary)]" />
        External app access
      </h2>
      <p class="text-sm text-[var(--text-secondary)] mb-4">
        Let other apps on this computer use your custom AIs for chat — with their
        persona, memory and conversation history.
      </p>

      {/* Status */}
      <div class="flex items-center gap-2 mb-4">
        <span class="inline-block w-2.5 h-2.5 rounded-full bg-emerald-500" />
        <span class="text-sm font-medium text-[var(--text-primary)]">On</span>
        <span class="text-sm text-[var(--text-secondary)]">
          — available while Your Own AI is open.
        </span>
      </div>

      {/* Endpoint + copy */}
      <div class="text-xs uppercase tracking-wide text-[var(--text-secondary)] mb-1.5">
        Local endpoint
      </div>
      <div class="flex items-center gap-2">
        <code class="flex-1 text-sm font-mono px-3 py-2 rounded-lg bg-[var(--bg-main)] border border-[var(--border-subtle)] text-[var(--text-primary)] truncate">
          {ENDPOINT}
        </code>
        <button
          onClick$={() => copy(ENDPOINT, "endpoint")}
          title="Copy endpoint"
          class="flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm border border-[var(--border-subtle)] text-[var(--text-secondary)] hover:bg-[var(--bg-main)] hover:text-[var(--text-primary)] transition-colors"
        >
          {copiedKey.value === "endpoint" ? (
            <LuCheck class="w-4 h-4 text-emerald-500" />
          ) : (
            <LuCopy class="w-4 h-4" />
          )}
          {copiedKey.value === "endpoint" ? "Copied" : "Copy"}
        </button>
      </div>

      {/* How to connect (disclosure) */}
      <button
        onClick$={() => (showHow.value = !showHow.value)}
        class="flex items-center gap-1.5 mt-4 text-sm text-[var(--text-link)] hover:underline"
      >
        <LuChevronDown
          class={`w-4 h-4 transition-transform ${showHow.value ? "" : "-rotate-90"}`}
        />
        How to connect an app
      </button>
      {showHow.value && (
        <div class="mt-3 text-sm text-[var(--text-secondary)] space-y-3 pl-1">
          <p>
            Point your app's OpenAI-compatible{" "}
            <span class="font-mono text-[var(--text-primary)]">base URL</span> at
            the endpoint above, and set the{" "}
            <span class="font-mono text-[var(--text-primary)]">model</span> to one
            of your AIs below. That AI's persona, memory and history all apply. No
            API key is needed on this computer — any value works.
          </p>

          {/* The exact model name to use for each AI (avoids confusion with
              multi-word names — use the hyphenated id, no spaces). */}
          {ais.value.length > 0 && (
            <div>
              <p class="text-xs uppercase tracking-wide text-[var(--text-secondary)] mb-2">
                Your AIs — use the model name on the right
              </p>
              <div class="space-y-1.5">
                {ais.value.map((ai) => (
                  <div
                    key={ai.model}
                    class="flex items-center justify-between gap-3 rounded-lg bg-[var(--bg-main)] border border-[var(--border-subtle)] px-3 py-2"
                  >
                    <span class="text-[var(--text-primary)] truncate">
                      {ai.name}
                    </span>
                    <div class="flex items-center gap-3 shrink-0">
                      <button
                        onClick$={() => copy(ai.model, ai.model)}
                        title="Copy just the model name"
                        class="flex items-center gap-1.5 font-mono text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-colors"
                      >
                        {ai.model}
                        {copiedKey.value === ai.model ? (
                          <LuCheck class="w-3.5 h-3.5 text-emerald-500" />
                        ) : (
                          <LuCopy class="w-3.5 h-3.5 opacity-60" />
                        )}
                      </button>
                      <button
                        onClick$={() => copy(setupSnippet(ai.model), `${ai.model}-setup`)}
                        title="Copy the base URL, model name, and a runnable example"
                        class="flex items-center gap-1 text-xs text-[var(--text-link)] hover:underline"
                      >
                        {copiedKey.value === `${ai.model}-setup` ? (
                          <>
                            <LuCheck class="w-3.5 h-3.5 text-emerald-500" />
                            Copied
                          </>
                        ) : (
                          "Copy setup"
                        )}
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          <p class="text-xs opacity-80">
            Works with OpenAI-compatible tools and agent frameworks.
          </p>
        </div>
      )}
    </section>
  );
});
