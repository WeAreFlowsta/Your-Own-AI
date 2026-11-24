/**
 * What stays on this computer and what an online AI is sent. Opened from
 * Memory and from the line under a reply that was made online. It states
 * the rule, lists what goes and what never goes, and shows each AI with
 * its switch one press away.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { type DocumentHead, useNavigate } from "@builder.io/qwik-city";
import { LuArrowLeft } from "@qwikest/icons/lucide";
import AppHeader from "../../components/AppHeader";
import { useHeaderWorkspace } from "../../hooks/useHeaderWorkspace";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import { getLocalCustomAis, updateLocalCustomAi } from "../../utils/localAiStorage";
import { canAnswerOnline } from "../../utils/memorySent";

interface Row {
  id: string;
  name: string;
  online: boolean;
  shared: boolean;
}

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const ais = useSignal<Row[]>([]);

  const load = $(async () => {
    const list = await getLocalCustomAis().catch(() => []);
    ais.value = list.map((a) => ({ id: a.id, name: a.name, online: canAnswerOnline(a.model), shared: a.memoryOnline !== false }));
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    await load();
  });

  const setShared = $(async (id: string, on: boolean) => {
    await updateLocalCustomAi(id, { memoryOnline: on }).catch(() => undefined);
    await load();
  });

  const row = "py-4 border-b border-[var(--border-color)] last:border-b-0";
  const head = "font-semibold text-[var(--text-primary)]";
  const body = "mt-1 text-sm text-[var(--text-secondary)]";
  const item = "mt-1 text-sm text-[var(--text-secondary)] list-disc pl-5 space-y-1";

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      <AppHeader
        currentModel={null}
        handleNewQuestion$={() => nav("/chat/")}
        handleModelsClick$={() => nav("/setup/")}
        folderPath={headerWs.folderPath.value}
        folderStatus={headerWs.folderStatus.value}
        permissionMode={headerWs.permissionMode.value}
        onCloseFolder$={headerWs.closeFolder$}
        buildInstalled={headerWs.buildInstalled.value}
        recentFolders={headerWs.recentFolders.value}
        onOpenFolder$={headerWs.openFolder$}
        onBrowseFolder$={headerWs.browseFolder$}
        onOpenConversations$={headerWs.openConversations$}
      />
      <div class="flex-1 overflow-y-auto">
        <div class="max-w-3xl mx-auto px-5 py-8" data-testid="memory-sent-page">
          <div class="flex items-center gap-3 mb-6">
            <LiquidMetalButton onClick$={() => nav("/your-memory/")} class="p-2">
              <LuArrowLeft class="w-5 h-5" />
            </LiquidMetalButton>
            <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela leading-tight">What stays on this computer and what an online AI is sent</h2>
          </div>

          <p class="text-sm text-[var(--text-secondary)]" data-testid="memory-sent-claim">
            Your memory is kept, searched and tidied on your own device, whatever model you use. An online AI is shown only what was found for the
            question you asked it, and you can see it and turn it off.
          </p>

          <div class="mt-4">
            <div class={row} data-testid="sent-row-stays">
              <div class={head}>Stays on this computer</div>
              <ul class={item}>
                <li>Your conversations, and the index that makes them searchable.</li>
                <li>The search itself, for every AI, local or online.</li>
                <li>Deciding what to keep, tidying, and checking that memory works.</li>
              </ul>
            </div>

            <div class={row} data-testid="sent-row-goes">
              <div class={head}>Sent with your message, when an AI answers online</div>
              <ul class={item}>
                <li>The passages from your conversations that the search found for that question, with their dates.</li>
                <li>What memory holds now about the things the question names.</li>
                <li>Your rules, and the rules of the project you are in.</li>
                <li>Your first name, when you have given one.</li>
                <li>The instruction to quote what it uses and give its date.</li>
              </ul>
              <p class={body}>The line under each online reply says how much was sent, and Show lists it.</p>
            </div>

            <div class={row} data-testid="sent-row-never">
              <div class={head}>Never sent</div>
              <ul class={item}>
                <li>Anything memory did not find for the question in hand.</li>
                <li>Anything that looks like a password or a key: it is covered before a passage is used.</li>
                <li>Anything from memory, when "Share memory" is off for that AI. Your rules still go.</li>
              </ul>
            </div>

            <div class={row} data-testid="sent-row-ais">
              <div class={head}>Your AIs</div>
              {ais.value.length === 0 && <p class={body}>No AIs yet.</p>}
              <ul class="mt-2 space-y-2">
                {ais.value.map((a) => (
                  <li key={a.id} class="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 text-sm" data-testid="sent-ai" data-online={a.online ? "1" : "0"} data-shared={a.shared ? "1" : "0"}>
                    <span class="text-[var(--text-secondary)]">
                      <span class="font-medium text-[var(--text-primary)]">{a.name}</span>
                      {a.online
                        ? a.shared
                          ? " can answer online. What memory finds for your question goes with your message."
                          : " can answer online. Memory is not shared with it."
                        : " runs on this computer. Nothing it is given leaves it."}
                    </span>
                    {a.online && (
                      <button
                        type="button"
                        data-testid="sent-ai-switch"
                        onClick$={() => setShared(a.id, !a.shared)}
                        class="text-xs font-medium text-[var(--text-link)] hover:underline"
                      >
                        {a.shared ? `Turn off for ${a.name}` : "Turn on"}
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "What an online AI is sent - Your Own AI",
};
