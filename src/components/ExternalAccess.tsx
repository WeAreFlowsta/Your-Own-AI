import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";
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
 *  needs (base URL + model + key when networked) plus a runnable example. */
function setupSnippet(model: string, base: string, key?: string): string {
  const lines = [`Base URL: ${base}`, `Model: ${model}`];
  if (key) lines.push(`API key: ${key}`);
  lines.push(
    "",
    "Example:",
    `curl ${base}/chat/completions \\`,
    `  -H "Content-Type: application/json" \\`,
  );
  if (key) lines.push(`  -H "Authorization: Bearer ${key}" \\`);
  lines.push(
    `  -d '{"model": "${model}", "messages": [{"role": "user", "content": "Hello"}]}'`,
  );
  return lines.join("\n");
}

interface LanStatus {
  enabled: boolean;
  key: string;
  ips: string[];
  port: number;
}

/**
 * Settings → External app access.
 *
 * This computer's apps connect keyless via localhost. The network toggle
 * additionally serves other devices on the local network - which must
 * present the auto-minted access key as their API key.
 */
export default component$(() => {
  const copiedKey = useSignal("");
  const showHow = useSignal(false);
  const ais = useSignal<{ name: string; model: string }[]>([]);
  const lan = useSignal<LanStatus>({ enabled: false, key: "", ips: [], port: 11435 });
  const lanBusy = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const list = await getLocalCustomAis();
      ais.value = list.map((a) => ({ name: a.name, model: modelId(a.name) }));
    } catch {
      /* none / store not ready */
    }
    try {
      lan.value = await invoke<LanStatus>("lan_access_status");
    } catch {
      /* status stays default */
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

  const toggleLan = $(async () => {
    if (lanBusy.value) return;
    lanBusy.value = true;
    try {
      lan.value = await invoke<LanStatus>("lan_access_set", {
        enabled: !lan.value.enabled,
      });
    } catch {
      /* keep previous state */
    } finally {
      lanBusy.value = false;
    }
  });

  const lanBase = () =>
    lan.value.ips.length > 0
      ? `http://${lan.value.ips[0]}:${lan.value.port}/v1`
      : `http://<this-computer's-address>:${lan.value.port}/v1`;

  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]">
      <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-1 flex items-center gap-2">
        <LuLink class="w-5 h-5 text-[var(--text-secondary)]" />
        External app access
      </h2>
      <p class="text-sm text-[var(--text-secondary)] mb-4">
        Let other apps use your custom AIs for chat — with their persona,
        memory and conversation history.
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
        On this computer
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
      <p class="mt-1.5 text-xs text-[var(--text-muted)]">
        Apps on this computer need no key — being here is the credential.
      </p>

      {/* Other devices on the network */}
      <div class="mt-5 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-main)] p-4">
        <div class="flex items-center justify-between gap-3">
          <div>
            <p class="text-sm font-medium text-[var(--text-primary)]">
              Other devices on this network
            </p>
            <p class="mt-0.5 text-xs text-[var(--text-secondary)]">
              Serve your AIs to your other machines — they connect with an
              access key.
            </p>
          </div>
          <button
            onClick$={toggleLan}
            disabled={lanBusy.value}
            class={`relative h-6 w-11 shrink-0 rounded-full transition-colors ${
              lan.value.enabled
                ? "bg-emerald-500"
                : "bg-[var(--border-subtle)]"
            }`}
            aria-label="Allow other devices on this network"
          >
            <span
              class={`absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all ${
                lan.value.enabled ? "left-[22px]" : "left-0.5"
              }`}
            />
          </button>
        </div>

        {lan.value.enabled && (
          <div class="mt-3 space-y-3">
            <div>
              <div class="text-xs uppercase tracking-wide text-[var(--text-secondary)] mb-1.5">
                From other devices
              </div>
              <div class="flex items-center gap-2">
                <code class="flex-1 text-sm font-mono px-3 py-2 rounded-lg bg-[var(--bg-card)] border border-[var(--border-subtle)] text-[var(--text-primary)] truncate">
                  {lanBase()}
                </code>
                <button
                  onClick$={() => copy(lanBase(), "lan-endpoint")}
                  title="Copy network endpoint"
                  class="flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm border border-[var(--border-subtle)] text-[var(--text-secondary)] hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)] transition-colors"
                >
                  {copiedKey.value === "lan-endpoint" ? (
                    <LuCheck class="w-4 h-4 text-emerald-500" />
                  ) : (
                    <LuCopy class="w-4 h-4" />
                  )}
                  {copiedKey.value === "lan-endpoint" ? "Copied" : "Copy"}
                </button>
              </div>
            </div>
            <div>
              <div class="text-xs uppercase tracking-wide text-[var(--text-secondary)] mb-1.5">
                Access key — paste as the API key
              </div>
              <div class="flex items-center gap-2">
                <code class="flex-1 text-sm font-mono px-3 py-2 rounded-lg bg-[var(--bg-card)] border border-[var(--border-subtle)] text-[var(--text-primary)] truncate">
                  {lan.value.key}
                </code>
                <button
                  onClick$={() => copy(lan.value.key, "lan-key")}
                  title="Copy access key"
                  class="flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm border border-[var(--border-subtle)] text-[var(--text-secondary)] hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)] transition-colors"
                >
                  {copiedKey.value === "lan-key" ? (
                    <LuCheck class="w-4 h-4 text-emerald-500" />
                  ) : (
                    <LuCopy class="w-4 h-4" />
                  )}
                  {copiedKey.value === "lan-key" ? "Copied" : "Copy"}
                </button>
                <button
                  onClick$={$(async () => {
                    try {
                      const key = await invoke<string>("lan_access_regenerate_key");
                      lan.value = { ...lan.value, key };
                    } catch {
                      /* keep old key */
                    }
                  })}
                  title="Make a new key - devices using the old one will need the new key"
                  class="px-3 py-2 rounded-lg text-sm border border-[var(--border-subtle)] text-[var(--text-secondary)] hover:bg-[var(--bg-card)] hover:text-[var(--text-primary)] transition-colors"
                >
                  New key
                </button>
              </div>
            </div>
            <p class="text-xs text-[var(--text-muted)]">
              Any device on this network with the key can chat with your AIs
              and read the replies — enable only on networks you trust. Your
              firewall may ask to allow Your Own AI the first time.
            </p>
          </div>
        )}
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
            of your AIs below. That AI's persona, memory and history all apply.
            {lan.value.enabled
              ? " From another device, use the network endpoint and paste the access key as the API key."
              : " No API key is needed on this computer — any value works."}
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
                        onClick$={() =>
                          copy(
                            setupSnippet(
                              ai.model,
                              lan.value.enabled ? lanBase() : ENDPOINT,
                              lan.value.enabled ? lan.value.key : undefined,
                            ),
                            `${ai.model}-setup`,
                          )
                        }
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
