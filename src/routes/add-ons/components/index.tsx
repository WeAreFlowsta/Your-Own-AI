/**
 * Add-ons > Components - the things this computer can get: engines, Projects,
 * the capability models (memory, vision, OCR), and the programs tools need.
 * Getting lives here; managing (location, sizes, remove) is Settings > Storage.
 */
import { component$, useSignal, useStore, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { LuBoxes, LuChevronLeft, LuCpu, LuChevronRight } from "@qwikest/icons/lucide";
import AppHeader from "../../../components/AppHeader";
import { useHeaderWorkspace } from "../../../hooks/useHeaderWorkspace";
import LiquidMetalButton from "../../../components/LiquidMetalButton";
import { Callout } from "../../../components/Callout";
import { ComponentCard, BuildComponentCard } from "../../../components/ComponentCards";
import { RequirementLine } from "../../../components/RequirementLine";
import { EMBEDDING_MODEL, UTILITY_MODEL, VISION_PROJECTORS, OCR_MODELS } from "../../../data/recommended-models";
import { whichProgram } from "../../../utils/mcp";

/** Programs tools ask for; the app installs these where it can. */
const RUNTIMES = [
  { program: "uv", label: "uv - runs Python tool servers", install: "https://docs.astral.sh/uv/getting-started/installation/" },
  { program: "git", label: "Git - fetches tool sources", install: "https://git-scm.com/downloads" },
  { program: "npx", label: "Node.js - runs JavaScript tool servers", install: "https://nodejs.org/en/download" },
  { program: "python3", label: "Python - some tools run on it directly", install: "https://www.python.org/downloads/" },
  { program: "docker", label: "Docker - tools that ship as containers", install: "https://docs.docker.com/get-docker/" },
];

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);
  const store = useStore({
    have: {} as Record<string, string | null | undefined>,
    engine: null as null | { supported: boolean; installed: boolean; tag: string },
    mlx: null as null | { supported: boolean; installed: boolean },
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    for (const r of RUNTIMES) store.have[r.program] = await whichProgram(r.program);
    try { store.engine = await invoke("engine_status"); } catch { /* no CUDA story on this machine */ }
    try { store.mlx = await invoke("mlx_engine_status"); } catch { /* not a Mac */ }
  });

  const handleNewQuestion = $(() => { nav("/chat"); });
  const handleModelsClick = $(() => { nav("/setup"); });

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
        <div class="max-w-4xl mx-auto px-4 py-8">
          <button type="button" onClick$={async () => { await nav("/add-ons"); }} class="inline-flex items-center gap-1 text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)]">
            <LuChevronLeft class="h-4 w-4" /> Add-ons
          </button>
          <h1 class="mt-2 flex items-center gap-2 text-2xl font-semibold text-[var(--text-primary)]">
            <LuBoxes class="h-6 w-6 text-[var(--text-secondary)]" /> Components
          </h1>
          <p class="mt-1 text-[var(--text-secondary)]">
            What this computer can get: engines for your hardware, Projects, the models behind memory, vision and OCR, and the programs tools need.
          </p>

          <Callout intent="info" title="Get here, manage in Settings" id="components-intro">
            Everything here downloads only when you ask, and says how big it is first. Where it lives, how much space it takes and removing it are in Settings › Storage.
          </Callout>

          <h2 class="mt-8 text-lg font-semibold text-[var(--text-primary)]">Projects</h2>
          <div class="mt-3"><BuildComponentCard /></div>

          <h2 class="mt-8 text-lg font-semibold text-[var(--text-primary)]">Engines</h2>
          <p class="mt-1 text-sm text-[var(--text-muted)]">The bundled engine runs on any GPU. Optional engines help specific hardware and are managed under Settings › Engines.</p>
          <button
            type="button"
            onClick$={async () => {
              try { sessionStorage.setItem("settings-from", JSON.stringify({ label: "Components", href: "/add-ons/components" })); } catch { /* fine */ }
              await nav("/settings#settings-engines");
            }}
            class="mt-3 flex w-full items-center gap-4 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 text-left transition-colors hover:border-[var(--text-link)]"
          >
            <div class="w-10 h-10 rounded-lg bg-[var(--bg-dropdown)] flex items-center justify-center flex-shrink-0">
              <LuCpu class="w-5 h-5 text-[var(--text-secondary)]" />
            </div>
            <div class="min-w-0 flex-1">
              <h3 class="text-base font-semibold text-[var(--text-primary)]">CUDA and MLX engines</h3>
              <p class="text-sm text-[var(--text-secondary)] mt-0.5">
                {store.engine?.supported
                  ? store.engine.installed ? "CUDA engine installed for your NVIDIA GPU." : "Your NVIDIA GPU can use the CUDA engine - faster reading of long questions."
                  : store.mlx?.supported
                    ? store.mlx.installed ? "MLX engine installed (preview)." : "This Mac can try the MLX engine (preview)."
                    : "Nothing extra needed for this machine."}
              </p>
            </div>
            <LuChevronRight class="h-5 w-5 text-[var(--text-muted)]" />
          </button>

          <h2 class="mt-8 text-lg font-semibold text-[var(--text-primary)]">Memory, vision and reading</h2>
          <div class="mt-3 flex flex-col gap-3">
            <ComponentCard model={EMBEDDING_MODEL} icon="brain" activeLabel="Installed - memory and smart routing are active." stopCommand="stop_embedding_server" />
            <ComponentCard model={UTILITY_MODEL} icon="spark" activeLabel="Installed - extraction and smart modes run on your device, even with online AIs." stopCommand="stop_utility_server" />
            {VISION_PROJECTORS.map((projector) => (
              <ComponentCard key={projector.id} model={projector} icon="eye" activeLabel="Installed - this model can now see images you attach." />
            ))}
            <ComponentCard model={OCR_MODELS} icon="scan" activeLabel="Installed - scanned PDFs are read on your device." />
          </div>

          <h2 class="mt-8 text-lg font-semibold text-[var(--text-primary)]">Programs tools need</h2>
          <p class="mt-1 text-sm text-[var(--text-muted)]">Tools in Add-ons ask for some of these. Install shows exactly what would run before it runs.</p>
          <ul class="mt-3 space-y-2 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4">
            {RUNTIMES.map((r) => {
              const program = r.program;
              return (
                <RequirementLine key={program} program={program} label={r.label} install={r.install} have={store.have[program]} onChange$={(v) => { store.have[program] = v; }} />
              );
            })}
          </ul>

          <div class="mt-8">
            <LiquidMetalButton variant="secondary" onClick$={async () => {
              try { sessionStorage.setItem("settings-from", JSON.stringify({ label: "Components", href: "/add-ons/components" })); } catch { /* fine */ }
              await nav("/settings#settings-components");
            }} class="h-9 px-4 text-sm">
              Manage storage in Settings
            </LiquidMetalButton>
          </div>
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Components - Your Own AI",
};
