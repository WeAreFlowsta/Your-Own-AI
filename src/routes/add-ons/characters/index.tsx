/**
 * Add-ons > Characters - complete AIs, ready to become yours: a personality,
 * a voice, a portrait and a starting memory, each a signed AI pack from
 * yourownai.net (the same eight as the site's "Your characters" page).
 * "Make it mine" fetches the pack, checks its signature, and creates the
 * AI exactly as Import AI does - it then lives in Your AIs, where it is
 * edited. The shelf stays as it is for the next one.
 */

import { component$, useSignal, useStore, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { invoke } from "@tauri-apps/api/core";
import { LuSparkles, LuChevronLeft, LuLoader, LuAlertTriangle, LuShieldCheck } from "@qwikest/icons/lucide";
import AppHeader from "../../../components/AppHeader";
import { useHeaderWorkspace } from "../../../hooks/useHeaderWorkspace";
import { useAiData, useAiDataActions } from "../../../contexts/AiDataContext";
import LiquidMetalButton from "../../../components/LiquidMetalButton";
import { Callout } from "../../../components/Callout";
import { parseAiPack, verifyAiPack, thumbnailDataUrlToBytes, type AiPack } from "../../../utils/aiPack";
import type { VerifyState } from "../../../utils/packSigning";
import { addKnowledge } from "../../../utils/transcriptMemory";
import type { UserDefinedAI } from "../../../types";

const SITE = "https://yourownai.net";

/** The shelf. Same eight as yourownai.net/uses/your-characters; the pack
 *  carries the description, persona, portrait and starting memory. */
const CHARACTERS: { slug: string; name: string }[] = [
  { slug: "maeve", name: "Maeve" },
  { slug: "rook", name: "Rook" },
  { slug: "sterling", name: "Sterling" },
  { slug: "emrys", name: "Emrys" },
  { slug: "patch", name: "Patch" },
  { slug: "ysolde", name: "Ysolde" },
  { slug: "juniper", name: "Juniper" },
  { slug: "cosmo", name: "Cosmo" },
];

interface ShelfEntry {
  slug: string;
  name: string;
  description: string;
  askBlurb: string;
  knowledgeCount: number;
  verify: VerifyState | null;
  pack: AiPack | null;
}

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const aiData = useAiData();
  const { createCustomAi, refreshThumbnail } = useAiDataActions();
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);

  const store = useStore({
    shelf: CHARACTERS.map((c) => ({ ...c, description: "", askBlurb: "", knowledgeCount: 0, verify: null, pack: null })) as ShelfEntry[],
    loading: true,
    offline: false,
    adding: "" as string,
    note: "" as string,
    error: "" as string,
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    currentModel.value = localStorage.getItem("currentModel");
    showModelWidget.value = localStorage.getItem("showModelWidget") === "true";
    // The packs are small (tens of KB) and carry the words for the cards;
    // fetch them all, verify each signature, keep going if one fails.
    let anyOk = false;
    await Promise.all(
      CHARACTERS.map(async (c, i) => {
        try {
          const res = await fetch(`${SITE}/characters/${c.slug}-pack.json`, { cache: "force-cache" });
          if (!res.ok) return;
          const pack = parseAiPack(await res.text());
          if (!pack) return;
          const verify = await verifyAiPack(pack);
          store.shelf[i] = {
            ...store.shelf[i],
            description: pack.description,
            askBlurb: pack.askBlurb ?? "",
            knowledgeCount: pack.knowledge.length,
            verify,
            pack,
          };
          anyOk = true;
        } catch {
          /* offline or blocked - the card shows the name and a retry */
        }
      }),
    );
    store.offline = !anyOk;
    store.loading = false;
  });

  const handleNewQuestion = $(() => {
    nav("/chat");
  });
  const handleModelsClick = $(() => {
    nav("/setup");
  });

  /** Same path as Import AI on Your AIs: create, portrait, starting memory. */
  const makeMine = $(async (slug: string) => {
    const entry = store.shelf.find((s) => s.slug === slug);
    const pack = entry?.pack;
    if (!pack || entry.verify === "tampered") return;
    store.error = "";
    store.note = "";
    store.adding = slug;
    try {
      const newAi = await createCustomAi({
        name: pack.name,
        description: pack.description,
        baseArchetypeId: pack.baseArchetypeId,
        systemPrompt: pack.systemPrompt,
        model: "auto:offline",
        askBlurb: pack.askBlurb,
        emoji: pack.emoji,
        useEmojis: pack.useEmojis,
        lengthDisposition: pack.lengthDisposition as UserDefinedAI["lengthDisposition"],
        defaultMode: pack.defaultMode as UserDefinedAI["defaultMode"],
      });
      if (pack.thumbnail) {
        const bytes = thumbnailDataUrlToBytes(pack.thumbnail);
        if (bytes) {
          try {
            await invoke("save_ai_thumbnail", { aiId: newAi.id, thumbnailData: bytes });
            await refreshThumbnail(newAi.id);
          } catch {
            /* portrait is a nice-to-have */
          }
        }
      }
      let added = 0;
      for (const e of pack.knowledge) {
        if (e.text.trim() && (await addKnowledge(newAi.id, e.text))) added++;
      }
      store.note = `${pack.name} joined your AIs${added ? ` with ${added} memor${added === 1 ? "y" : "ies"} to start from` : ""}. Find ${pack.name} in Your AIs.`;
    } catch (e) {
      console.error("[Characters] pack import failed:", e);
      store.error = `Couldn't add ${pack.name} - please try again.`;
    } finally {
      store.adding = "";
    }
  });

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      <div class="relative z-20">
        <AppHeader
          handleNewQuestion$={handleNewQuestion}
          handleModelsClick$={handleModelsClick}
          currentModel={currentModel.value}
          folderPath={headerWs.folderPath.value}
          folderStatus={headerWs.folderStatus.value}
          permissionMode={headerWs.permissionMode.value}
          onCloseFolder$={headerWs.closeFolder$}
          buildInstalled={headerWs.buildInstalled.value}
          recentFolders={headerWs.recentFolders.value}
          onOpenFolder$={headerWs.openFolder$}
          onBrowseFolder$={headerWs.browseFolder$}
          onOpenConversations$={headerWs.openConversations$}
          showModelWidget={showModelWidget.value && currentModel.value !== null}
        />
      </div>

      <div class="flex-1 overflow-y-auto">
        <div class="max-w-5xl mx-auto px-4 py-8">
          <button
            type="button"
            onClick$={async () => {
              await nav("/add-ons");
            }}
            class="inline-flex items-center gap-1 text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
          >
            <LuChevronLeft class="h-4 w-4" /> Add-ons
          </button>
          <h1 class="mt-2 flex items-center gap-2 text-2xl font-semibold text-[var(--text-primary)]">
            <LuSparkles class="h-6 w-6 text-[var(--text-secondary)]" /> Characters
          </h1>
          <p class="mt-1 text-[var(--text-secondary)]">
            Complete AIs, ready to become yours: a personality, a voice, a portrait and a starting memory. Made by
            Flowsta, signed, free.
          </p>

          <Callout intent="info" title="A character becomes your AI" id="characters-intro">
            "Make it mine" creates a new AI from the character - persona, portrait and starting memories - and it
            lives in Your AIs from then on: rename it, change its model, give it skills and knowledge. The character
            on this shelf stays as it is for the next one. Your own AIs can be shared the same way: Export AI on
            any card.
          </Callout>

          {store.note && (
            <div class="mt-4 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-2 text-sm text-[var(--text-primary)]">
              {store.note}{" "}
              <button type="button" class="text-[var(--text-link)] hover:underline" onClick$={async () => { await nav("/your-ais"); }}>
                Open Your AIs
              </button>
            </div>
          )}
          {store.error && (
            <div class="mt-4 flex items-start gap-2 rounded-xl border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-[var(--text-primary)]">
              <LuAlertTriangle class="mt-0.5 h-4 w-4 shrink-0 text-red-500" />
              <span>{store.error}</span>
            </div>
          )}
          {!store.loading && store.offline && (
            <div class="mt-4 flex items-start gap-2 rounded-xl border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-[var(--text-primary)]">
              <LuAlertTriangle class="mt-0.5 h-4 w-4 shrink-0 text-amber-500" />
              <span>The characters live on yourownai.net and need a connection to fetch (a few tens of KB each). Everything else here works offline.</span>
            </div>
          )}

          <div class="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {store.shelf.map((c) => {
              const owned = aiData.userDefinedAis.some((a) => a.name === c.name && a.status !== "archived");
              return (
                <div
                  key={c.slug}
                  class="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 flex flex-col gap-3"
                >
                  <div class="flex items-center gap-3">
                    <div class="h-14 w-14 shrink-0 overflow-hidden rounded-full border border-[var(--border-subtle)] bg-[var(--bg-main)]">
                      <img src={`${SITE}/characters/${c.slug}.jpg`} alt="" width={56} height={56} class="h-full w-full object-cover" loading="lazy" />
                    </div>
                    <div class="min-w-0">
                      <h2 class="truncate font-medium text-[var(--text-primary)]">{c.name}</h2>
                      <p class="flex items-center gap-1 text-xs text-[var(--text-muted)]">
                        {c.verify === "verified" && <LuShieldCheck class="h-3.5 w-3.5 text-emerald-500" />}
                        {c.verify === "verified" ? "Made by Flowsta, signed" : c.verify === "tampered" ? "Signature does not match" : "Made by Flowsta"}
                      </p>
                    </div>
                  </div>
                  <p class="text-sm text-[var(--text-secondary)] line-clamp-3 flex-1">
                    {c.description || (store.loading ? "Loading…" : "Could not fetch this character right now.")}
                  </p>
                  {c.pack && (
                    <p class="text-xs text-[var(--text-muted)]">
                      {c.askBlurb ? `Ask ${c.name} ${c.askBlurb}` : ""}
                      {c.askBlurb && c.knowledgeCount ? " · " : ""}
                      {c.knowledgeCount ? `${c.knowledgeCount} starting memor${c.knowledgeCount === 1 ? "y" : "ies"}` : ""}
                    </p>
                  )}
                  <div class="flex items-center justify-between gap-2">
                    <span class="text-xs text-[var(--text-muted)]">{owned ? `${c.name} is already one of your AIs` : ""}</span>
                    <LiquidMetalButton
                      variant={owned ? "secondary" : "primary"}
                      class="flex items-center gap-1.5 h-9 px-4 sm:px-5 text-sm"
                      disabled={!c.pack || c.verify === "tampered" || !!store.adding}
                      onClick$={() => makeMine(c.slug)}
                    >
                      {store.adding === c.slug && <LuLoader class="h-4 w-4 animate-spin" />}
                      {owned ? "Make another" : "Make it mine"}
                    </LiquidMetalButton>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Characters - Your Own AI",
};
