/**
 * Per-AI authored knowledge ("packs/lore"): things the user gives a specific
 * AI to know — distinct from what it learns in chat (AiEpisodicMemory) and from
 * the shared profile (ProfileMemory). Entries are embedded and recalled when
 * relevant. A collection can be exported/imported as a JSON file — the
 * shareable "pack". Lives in the Remembers tab.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { readThroughWarmup } from "../utils/recordsWarmup";
import { invoke } from "@tauri-apps/api/core";
import {
  LuBookOpen,
  LuPlus,
  LuTrash2,
  LuDownload,
  LuUpload,
  LuShieldCheck,
  LuShieldX,
} from "@qwikest/icons/lucide";
import {
  getAiKnowledge,
  addKnowledge,
  deleteAiMemory,
  type TranscriptEmbedding,
} from "../utils/transcriptMemory";
import { isEmbeddingModelReady } from "../utils/embeddings";
import LiquidMetalButton from "./LiquidMetalButton";
import {
  vaultState,
  signPack,
  verifyPack,
  signInToFlowsta,
  type KnowledgePack,
  type VerifyState,
} from "../utils/packSigning";

interface Props {
  aiId: string;
  aiName?: string;
}

/** Filename for an exported pack (.json). */
function packFilename(pack: KnowledgePack): string {
  return `${(pack.name || "ai").replace(/[^a-z0-9]+/gi, "-").toLowerCase()}.json`;
}

export default component$<Props>(({ aiId, aiName }) => {
  const items = useSignal<TranscriptEmbedding[]>([]);
  const loading = useSignal(true);
  const modelReady = useSignal(true);
  const showAdd = useSignal(false);
  const addValue = useSignal("");
  const busy = useSignal(false);
  const note = useSignal("");

  // Export
  const exportOpen = useSignal(false);
  const exporting = useSignal(false);
  const exportErr = useSignal("");
  const vaultUnlocked = useSignal(false);
  const vaultInstalled = useSignal(false);
  // Import
  const importOpen = useSignal(false);
  const importPack = useSignal<KnowledgePack | null>(null);
  const importVerify = useSignal<VerifyState | null>(null);
  const importing = useSignal(false);

  const warming = useSignal(false);
  const load = $(async () => {
    if (!aiId) {
      loading.value = false;
      return;
    }
    modelReady.value = await isEmbeddingModelReady();
    // Knowledge decrypts with a key the conductor holds - early reads
    // come back empty while it starts; hold the line rather than show
    // a false "nothing yet".
    items.value = await readThroughWarmup(
      () => getAiKnowledge(aiId),
      (w) => (warming.value = w),
    );
    loading.value = false;
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    track(() => aiId);
    loading.value = true;
    await load();
  });

  const doAdd = $(async () => {
    const v = addValue.value.trim();
    if (!v) return;
    busy.value = true;
    const ok = await addKnowledge(aiId, v);
    busy.value = false;
    if (!ok) {
      note.value = "Couldn't save — the memory recall model is needed.";
      return;
    }
    addValue.value = "";
    showAdd.value = false;
    note.value = "";
    await load();
  });

  const doDelete = $(async (id: string) => {
    await deleteAiMemory(aiId, id);
    items.value = items.value.filter((m) => m.id !== id);
  });

  const buildPack = $(async (): Promise<KnowledgePack> => {
    const entries = (await getAiKnowledge(aiId)).map((e) => ({ text: e.text }));
    return {
      format: "flowsta-knowledge",
      version: 1,
      name: `${aiName || "AI"} knowledge`,
      entries,
    };
  });

  const openExport = $(async () => {
    exportErr.value = "";
    const vs = await vaultState();
    vaultInstalled.value = vs.installed;
    vaultUnlocked.value = vs.unlocked;
    exportOpen.value = true;
  });

  const doExportUnsigned = $(async () => {
    exporting.value = true;
    exportErr.value = "";
    try {
      const pack = await buildPack();
      const path = await invoke<string>("save_text_download", {
        filename: packFilename(pack),
        content: JSON.stringify(pack, null, 2),
      });
      exportOpen.value = false;
      note.value = `Pack saved to ${path}`;
    } catch {
      exportErr.value = "Couldn't save the file.";
    } finally {
      exporting.value = false;
    }
  });

  const doExportSigned = $(async () => {
    exporting.value = true;
    exportErr.value = "";
    try {
      const pack = await buildPack();
      const signed = { ...pack, signature: await signPack(pack) };
      const path = await invoke<string>("save_text_download", {
        filename: packFilename(signed),
        content: JSON.stringify(signed, null, 2),
      });
      exportOpen.value = false;
      note.value = `Signed pack saved to ${path}`;
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      exportErr.value =
        m === "vault_locked"
          ? "Flowsta Vault is locked — unlock it to sign."
          : m === "vault_not_found"
            ? "Flowsta Vault isn't running."
            : "Couldn't sign or save the pack.";
    } finally {
      exporting.value = false;
    }
  });

  const doSignIn = $(async () => {
    exporting.value = true;
    exportErr.value = "";
    try {
      await signInToFlowsta();
      const vs = await vaultState();
      vaultInstalled.value = vs.installed;
      vaultUnlocked.value = vs.unlocked;
      if (!vaultUnlocked.value) exportErr.value = "Sign-in didn't complete.";
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      exportErr.value =
        m === "vault_not_found"
          ? "Flowsta Vault isn't installed or running."
          : m === "vault_locked"
            ? "Unlock Flowsta Vault first."
            : "Sign-in was cancelled or didn't complete.";
    } finally {
      exporting.value = false;
    }
  });

  const onFilePicked = $(async (file: File) => {
    note.value = "";
    try {
      const data = JSON.parse(await file.text());
      const rawEntries = Array.isArray(data)
        ? data
        : Array.isArray(data?.entries)
          ? data.entries
          : [];
      const entries = rawEntries
        .map((e: unknown) =>
          typeof e === "string" ? { text: e } : { text: (e as { text?: string })?.text ?? "" },
        )
        .filter((e: { text: string }) => e.text.trim().length > 0);
      if (entries.length === 0) {
        note.value = "No items found in that file.";
        return;
      }
      const pack: KnowledgePack = {
        format: data?.format ?? "flowsta-knowledge",
        version: data?.version ?? 1,
        name: data?.name,
        entries,
        signature: data?.signature,
      };
      importVerify.value = await verifyPack(pack);
      importPack.value = pack;
      importOpen.value = true;
    } catch {
      note.value = "Couldn't read that file — expected a knowledge .json pack.";
    }
  });

  const doConfirmImport = $(async () => {
    const pack = importPack.value;
    if (!pack || importVerify.value === "tampered") return;
    importing.value = true;
    let added = 0;
    for (const e of pack.entries) {
      if (e.text.trim() && (await addKnowledge(aiId, e.text))) added++;
    }
    importing.value = false;
    importOpen.value = false;
    importPack.value = null;
    await load();
    note.value = `Imported ${added} item${added === 1 ? "" : "s"}.`;
  });

  const name = aiName || "this AI";

  return (
    <div>
      <div class="flex flex-wrap items-center justify-between gap-3 mb-4">
        <p class="text-sm text-[var(--text-secondary)] flex items-center gap-1.5">
          <LuBookOpen class="w-3.5 h-3.5 text-[var(--text-muted)] flex-shrink-0" />
          Knowledge you've given {name} —{" "}
          <span class="text-[var(--text-muted)]">only this AI</span>
        </p>
        {modelReady.value && (
          <div class="flex items-center gap-2">
            {/* Secondary actions first, primary (Add) on the right. */}
            {/* A <label> (triggers the hidden file input) wearing the secondary
                liquid-metal pill, so it matches the Add/Export buttons beside it. */}
            <label
              for="knowledge-import-input"
              title="Import a knowledge pack (.json)"
              class="btn-liquid-metal btn-secondary-metal cursor-pointer flex items-center gap-1.5 px-3 py-1.5 text-xs"
            >
              <div class="shader-border" />
              <div class="shader-inner-fill" />
              <span class="btn-content">
                <LuUpload class="w-3.5 h-3.5" /> Import
              </span>
            </label>
            {items.value.length > 0 && (
              <LiquidMetalButton
                variant="secondary"
                onClick$={openExport}
                title="Export as a shareable knowledge pack (.json)"
                class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
              >
                <LuDownload class="w-3.5 h-3.5" /> Export
              </LiquidMetalButton>
            )}
            <LiquidMetalButton
              onClick$={() => (showAdd.value = !showAdd.value)}
              class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
            >
              <LuPlus class="w-3.5 h-3.5" /> Add
            </LiquidMetalButton>
          </div>
        )}
      </div>

      <input
        id="knowledge-import-input"
        type="file"
        accept="application/json,.json"
        class="hidden"
        onChange$={(_, el) => {
          const f = el.files?.[0];
          if (f) onFilePicked(f);
          el.value = "";
        }}
      />

      {showAdd.value && (
        <div class="mb-4 p-4 rounded-xl bg-[var(--bg-card)] border border-[var(--border-subtle)]">
          <textarea
            value={addValue.value}
            rows={2}
            placeholder={`e.g. "You were born under a great oak by the river"  ·  "Our refund window is 30 days"`}
            onInput$={(_, el) => (addValue.value = el.value)}
            onKeyDown$={(e) => {
              if (e.key === "Escape") {
                showAdd.value = false;
                addValue.value = "";
              }
            }}
            autoFocus
            class="w-full px-3 py-2 rounded-lg bg-[var(--bg-main)] border border-[var(--border-subtle)] text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--bg-button-primary)] resize-y"
          />
          <p class="mt-1.5 text-[11px] text-[var(--text-muted)]">
            Tip: write things about {name} itself as{" "}
            <span class="text-[var(--text-secondary)]">"You…"</span> (e.g. "You
            distrust the merchant guild" · "You always cite your sources"). Plain
            facts and policies can stay as-is.
          </p>
          <div class="flex items-center justify-end gap-2 mt-2">
            <LiquidMetalButton
              variant="secondary"
              onClick$={() => {
                showAdd.value = false;
                addValue.value = "";
              }}
              class="px-3 py-1.5 text-sm"
            >
              Cancel
            </LiquidMetalButton>
            <LiquidMetalButton
              onClick$={doAdd}
              disabled={!addValue.value.trim() || busy.value}
              class="px-4 py-1.5 text-sm"
            >
              Add
            </LiquidMetalButton>
          </div>
        </div>
      )}

      {note.value && (
        <p class="mb-3 text-xs text-[var(--text-secondary)]">{note.value}</p>
      )}

      {loading.value ? (
        <div class="text-center py-8">
          <div class="inline-block w-6 h-6 border-4 border-[var(--border-subtle)] border-t-[var(--bg-button-primary)] rounded-full animate-spin"></div>
          {warming.value && (
            <p class="mt-3 text-sm text-[var(--text-muted)] max-w-md mx-auto">
              Your records are warming up - just after launch, its knowledge takes a moment to be ready.
            </p>
          )}
        </div>
      ) : !modelReady.value ? (
        <div class="text-center py-10 rounded-2xl border border-dashed border-[var(--border-subtle)]">
          <LuBookOpen class="w-9 h-9 mx-auto mb-3 text-[var(--text-muted)]" />
          <p class="text-sm text-[var(--text-secondary)] max-w-md mx-auto flex flex-col items-center gap-2">
            <span>Giving an AI knowledge needs the memory recall model.</span>
            <a
              href="/settings"
              class="inline-flex items-center gap-1.5 text-[var(--text-link)] hover:underline"
            >
              <LuDownload class="w-3.5 h-3.5" /> Get it in Settings → Components
            </a>
          </p>
        </div>
      ) : items.value.length === 0 ? (
        <div class="text-center py-10 rounded-2xl border border-dashed border-[var(--border-subtle)]">
          <LuBookOpen class="w-9 h-9 mx-auto mb-3 text-[var(--text-muted)]" />
          <p class="text-sm text-[var(--text-secondary)] max-w-md mx-auto">
            Give {name} knowledge only it should have — a character's backstory,
            or your products, policies, and procedures. Add your own or import a
            pack; it'll draw on them when relevant.
          </p>
        </div>
      ) : (
        <div class="space-y-2">
          {items.value.map((m) => (
            <div
              key={m.id}
              class="group flex items-start gap-3 p-3 rounded-xl bg-[var(--bg-card)] border border-[var(--border-subtle)]"
            >
              <LuBookOpen class="w-4 h-4 text-[var(--text-muted)] flex-shrink-0 mt-0.5" />
              <p class="min-w-0 flex-1 text-sm text-[var(--text-primary)] break-words">
                {m.text}
              </p>
              <LiquidMetalButton
                variant="danger"
                onClick$={() => doDelete(m.id)}
                title="Remove"
                class="p-1 flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
              >
                <LuTrash2 class="w-3.5 h-3.5" />
              </LiquidMetalButton>
            </div>
          ))}
        </div>
      )}

      {/* Export options */}
      {exportOpen.value && (
        <div
          class="fixed inset-0 bg-black/60 flex items-center justify-center p-4 z-50"
          onClick$={() => !exporting.value && (exportOpen.value = false)}
        >
          <div
            class="bg-[var(--bg-header-footer)] p-6 rounded-xl shadow-2xl w-full max-w-md"
            onClick$={(e: MouseEvent) => e.stopPropagation()}
          >
            <h2 class="text-lg font-semibold text-[var(--text-primary)] font-varela mb-1">
              Export knowledge pack
            </h2>
            <p class="text-sm text-[var(--text-secondary)] mb-4">
              {vaultUnlocked.value
                ? "Sign it with your Flowsta identity so others can verify it's from you and hasn't been changed — or export it unsigned."
                : vaultInstalled.value
                  ? "Your Flowsta Vault is locked. Unlock it to sign this pack — or export it unsigned."
                  : "Signing needs the Flowsta Vault app on this computer. You can still export unsigned."}
            </p>
            {exportErr.value && (
              <p class="text-xs text-red-400 mb-3">{exportErr.value}</p>
            )}
            <div class="flex flex-col gap-2">
              {vaultUnlocked.value ? (
                <LiquidMetalButton
                  onClick$={doExportSigned}
                  disabled={exporting.value}
                  class="flex items-center justify-center gap-2 px-4 py-2 text-sm"
                >
                  {exporting.value && (
                    <div class="w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin" />
                  )}
                  <LuShieldCheck class="w-4 h-4" /> Export signed
                </LiquidMetalButton>
              ) : vaultInstalled.value ? (
                <LiquidMetalButton
                  onClick$={doSignIn}
                  disabled={exporting.value}
                  class="flex items-center justify-center gap-2 px-4 py-2 text-sm"
                >
                  {exporting.value && (
                    <div class="w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin" />
                  )}
                  I've unlocked it — sign in
                </LiquidMetalButton>
              ) : (
                <LiquidMetalButton
                  onClick$={$(async () => {
                    const { openUrl } = await import("@tauri-apps/plugin-opener");
                    await openUrl("https://flowsta.com/vault/");
                  })}
                  disabled={exporting.value}
                  class="flex items-center justify-center gap-2 px-4 py-2 text-sm"
                >
                  Get Flowsta Vault
                </LiquidMetalButton>
              )}
              <LiquidMetalButton
                variant="secondary"
                onClick$={doExportUnsigned}
                disabled={exporting.value}
                class="px-4 py-2 text-sm"
              >
                Export unsigned
              </LiquidMetalButton>
              <LiquidMetalButton
                variant="secondary"
                onClick$={() => (exportOpen.value = false)}
                disabled={exporting.value}
                class="px-4 py-2 text-sm"
              >
                Cancel
              </LiquidMetalButton>
            </div>
          </div>
        </div>
      )}

      {/* Import — verify + confirm */}
      {importOpen.value && importPack.value && (
        <div
          class="fixed inset-0 bg-black/60 flex items-center justify-center p-4 z-50"
          onClick$={() => !importing.value && (importOpen.value = false)}
        >
          <div
            class="bg-[var(--bg-header-footer)] p-6 rounded-xl shadow-2xl w-full max-w-md"
            onClick$={(e: MouseEvent) => e.stopPropagation()}
          >
            {importVerify.value === "verified" ? (
              <div class="flex items-start gap-3">
                <div class="w-10 h-10 rounded-full bg-emerald-500/10 text-emerald-400 flex items-center justify-center flex-shrink-0">
                  <LuShieldCheck class="w-5 h-5" />
                </div>
                <div class="min-w-0">
                  <h2 class="text-lg font-semibold text-[var(--text-primary)] font-varela">
                    Signed &amp; verified
                  </h2>
                  <p class="text-sm text-[var(--text-secondary)] mt-1">
                    This pack is signed and hasn't been changed since. Import{" "}
                    {importPack.value.entries.length} item
                    {importPack.value.entries.length === 1 ? "" : "s"} into {name}?
                  </p>
                  <p class="text-[11px] text-[var(--text-muted)] mt-2 break-all">
                    Signer: {importPack.value.signature?.agent_pub_key.slice(0, 22)}…
                  </p>
                </div>
              </div>
            ) : importVerify.value === "tampered" ? (
              <div class="flex items-start gap-3">
                <div class="w-10 h-10 rounded-full bg-red-500/10 text-red-400 flex items-center justify-center flex-shrink-0">
                  <LuShieldX class="w-5 h-5" />
                </div>
                <div class="min-w-0">
                  <h2 class="text-lg font-semibold text-[var(--text-primary)] font-varela">
                    Can't import this pack
                  </h2>
                  <p class="text-sm text-[var(--text-secondary)] mt-1">
                    It carries a signature, but it doesn't match its contents — it
                    may have been altered since it was signed. For safety,
                    importing is blocked.
                  </p>
                </div>
              </div>
            ) : (
              <div>
                <h2 class="text-lg font-semibold text-[var(--text-primary)] font-varela">
                  Import knowledge pack
                </h2>
                <p class="text-sm text-[var(--text-secondary)] mt-1">
                  Import {importPack.value.entries.length} item
                  {importPack.value.entries.length === 1 ? "" : "s"} into {name}?
                </p>
                <p class="text-xs text-[var(--text-secondary)] mt-2 p-2 rounded-lg bg-[var(--bg-card)] border border-[var(--border-subtle)]">
                  This pack is unsigned, so you can't verify who made it. Packs
                  signed with Flowsta let you confirm the source.
                </p>
              </div>
            )}
            <div class="flex justify-end gap-2 mt-5">
              <LiquidMetalButton
                variant="secondary"
                onClick$={() => (importOpen.value = false)}
                disabled={importing.value}
                class="px-4 py-2 text-sm"
              >
                {importVerify.value === "tampered" ? "Close" : "Cancel"}
              </LiquidMetalButton>
              {importVerify.value !== "tampered" && (
                <LiquidMetalButton
                  onClick$={doConfirmImport}
                  disabled={importing.value}
                  class="flex items-center gap-2 px-4 py-2 text-sm"
                >
                  {importing.value && (
                    <div class="w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin" />
                  )}
                  Import
                </LiquidMetalButton>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
});
