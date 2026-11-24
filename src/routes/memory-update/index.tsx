/**
 * What changed in the update: where each thing from the older memory
 * went, one row each. Opened from the line shown after the update and
 * from Memory at any time. Nothing here has to be answered.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { type DocumentHead, useNavigate } from "@builder.io/qwik-city";
import { invoke } from "@tauri-apps/api/core";
import { LuArrowLeft } from "@qwikest/icons/lucide";
import AppHeader from "../../components/AppHeader";
import { useHeaderWorkspace } from "../../hooks/useHeaderWorkspace";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import { updateReport, type UpdateReport } from "../../utils/memoryUpdate";

interface Reading {
  read: number;
  total: number;
}

const n = (count: number, one: string, many: string) => `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const report = useSignal<UpdateReport | null>(null);
  const reading = useSignal<Reading | null>(null);
  const loaded = useSignal(false);
  const readNow = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    report.value = await updateReport();
    const poll = async () => {
      reading.value = await invoke<Reading>("memory_reading_status").catch(() => null);
    };
    await poll();
    loaded.value = true;
    const t = setInterval(poll, 15_000);
    cleanup(() => clearInterval(t));
  });

  const startReading = $(async () => {
    readNow.value = true;
    await invoke("memory_read_now", { on: true }).catch(() => undefined);
  });

  const r = report.value;
  const rd = reading.value;
  const percent = rd && rd.total > 0 ? Math.min(100, Math.floor((rd.read / rd.total) * 100)) : null;
  const row = "py-4 border-b border-[var(--border-color)] last:border-b-0";
  const head = "font-semibold text-[var(--text-primary)]";
  const body = "mt-1 text-sm text-[var(--text-secondary)]";

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
        <div class="max-w-3xl mx-auto px-5 py-8" data-testid="memory-update-page">
          <div class="flex items-center gap-3 mb-6">
            <LiquidMetalButton onClick$={() => nav("/your-memory/")} class="p-2">
              <LuArrowLeft class="w-5 h-5" />
            </LiquidMetalButton>
            <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela leading-tight">What changed in the update</h2>
          </div>

          {loaded.value && !r && (
            <p class="text-sm text-[var(--text-secondary)]" data-testid="memory-update-pending">
              The update is still going through what was here before. This page fills in when it has finished. You can chat meanwhile.
            </p>
          )}

          <div>
            <div class={row} data-testid="update-row-conversations">
              <div class={head}>Your conversations</div>
              <p class={body}>
                All here.{" "}
                {percent === null
                  ? "They are read in idle time so your AIs can search them, newest first."
                  : percent >= 100
                    ? "All of them have been read, so your AIs can search every one."
                    : `They are being read so your AIs can search them, newest first: ${percent}% so far. You can chat meanwhile, and finding already works on what is read.`}
              </p>
            </div>

            {r && r.keptByYou > 0 && (
              <div class={row} data-testid="update-row-kept">
                <div class={head}>{n(r.keptByYou, "thing you saved with Remember", "things you saved with Remember")}</div>
                <p class={body}>In Memory, each marked "Kept by you" with its date and the AI it was saved for. Used as before, now by all your AIs.</p>
              </div>
            )}

            {r && r.setAside > 0 && (
              <div class={row} data-testid="update-row-set-aside">
                <div class={head}>{n(r.setAside, "fact your AIs had learned about you", "facts your AIs had learned about you")}</div>
                <p class={body}>
                  Set aside. They are listed under About you as "Learned before the update, not used". Your AIs no longer use them unless you press Keep on one.
                  {r.name ? " Your name keeps working." : ""}
                </p>
              </div>
            )}

            {r && r.portraitRetired && (
              <div class={row} data-testid="update-row-portrait">
                <div class={head}>The short description of you</div>
                <p class={body}>No longer sent to any AI.</p>
              </div>
            )}

            <div class={row} data-testid="update-row-recalled">
              <div class={head}>Each AI's list of what it recalled</div>
              <p class={body}>
                Gone, because whole conversations can be searched now. If you had removed a line from one of those lists, the conversation it came from can be found again now. To keep a conversation out, open it and press "Do not use this conversation".
              </p>
            </div>

            <div class={row} data-testid="update-row-documents">
              <div class={head}>Documents you gave an AI</div>
              <p class={body}>Where they were, with each AI.</p>
            </div>

            <div class={row} data-testid="update-row-rules">
              <div class={head}>Your rules</div>
              <p class={body}>Given to every AI with every message, as before.</p>
            </div>

            {r && r.projectLines > 0 && (
              <div class={row} data-testid="update-row-projects">
                <div class={head}>Project memory</div>
                <p class={body}>
                  Now part of Memory. {n(r.projectLines, "note", "notes")} from {n(r.projects, "project", "projects")} are items in their project, marked as kept. What an AI must be told every time in a project is under "Rules for this project".
                </p>
              </div>
            )}

            {r && r.tidyingStartsOff && (
              <div class={row} data-testid="update-row-tidying">
                <div class={head}>Automatic learning</div>
                <p class={body}>
                  It is now called tidying. You had Automatic learning turned off, so tidying starts off and your AIs keep nothing by themselves. Finding works, and Remember works.
                </p>
              </div>
            )}

            <div class={row} data-testid="update-row-old-conversations">
              <div class={head}>Old conversations</div>
              <p class={body}>
                Nothing is gone through in bulk to guess what matters. They are all searched, and when one answers a question it is kept then, so Memory fills in as you use it.
              </p>
            </div>
          </div>

          <div class="mt-6 flex items-center justify-end gap-3">
            {percent !== null && percent < 100 && (
              <LiquidMetalButton onClick$={startReading} class="px-4 py-2 text-sm" testId="update-read-now">
                {readNow.value ? "Reading now" : "Read my conversations now"}
              </LiquidMetalButton>
            )}
            <LiquidMetalButton onClick$={() => nav("/your-memory/")} class="px-4 py-2 text-sm" testId="update-go-memory">
              Go to Memory
            </LiquidMetalButton>
          </div>
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = { title: "What changed in the update - Your Own AI" };
