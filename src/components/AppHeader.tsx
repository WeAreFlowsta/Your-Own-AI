/**
 * App Header - Qwik version (migrated from React)
 * Simplified for local-first app (no authentication)
 */

import {
  component$,
  useContext,
  useSignal,
  useVisibleTask$,
  $,
  type QRL,
} from "@builder.io/qwik";
import { useNavigate } from "@builder.io/qwik-city";
import {
  LuSettings2,
  LuSettings,
  LuSun,
  LuMoon,
  LuCheck,
  LuBot,
  LuFolderOpen,
  LuMessagesSquare,
  LuBrain,
  LuDownload,
  LuCloud,
  LuX,
  LuRotateCw,
  LuPuzzle,
} from "@qwikest/icons/lucide";

import { ThemeContext, ProjectMemoryContext, type AppTheme } from "../routes/layout";
import {
  fetchUsage,
  formatUsage,
  usageTickerEnabled,
  type UsageSummary,
} from "../utils/usagePrefs";

import LiquidMetalButton from "./LiquidMetalButton";
import { Callout } from "./Callout";
import logoLight from "../assets/logo-light.svg";
import logo from "../assets/logo.svg";
import logoSymbolLight from "../assets/logo-symbol-light.svg";
import logoSymbol from "../assets/logo-symbol.svg";

/**
 * Logo sub-component.
 * React.memo is not needed in Qwik — fine-grained reactivity handles it.
 */
const Logo = component$<{ theme: AppTheme }>(({ theme }) => {
  const useLightLogo = theme === "dark";
  const fullLogo = useLightLogo ? logoLight : logo;
  const symbolLogo = useLightLogo ? logoSymbolLight : logoSymbol;

  return (
    <>
      <img
        src={fullLogo}
        alt="Your Own AI"
        class="h-9 hidden sm:block"
        style={{ width: "auto" }}
      />
      <img
        src={symbolLogo}
        alt="Your Own AI"
        class="h-9 w-auto block sm:hidden"
      />
    </>
  );
});

interface AppHeaderProps {
  handleNewQuestion$: QRL<() => void>;
  handleModelsClick$: QRL<() => void>;
  currentModel: string | null;
  isModelLoading?: boolean;
  modelTooBig?: boolean;
  /// Why the chip is red when it is: "too big" | "not loaded" | "can't be read" | "engine crashed".
  modelIssue?: string;
  showModelWidget?: boolean;
  /** The app-wide workspace folder - null = none open. */
  folderPath?: string | null;
  /** Agent session status while a folder is open. */
  folderStatus?: 'starting' | 'ready' | 'working' | 'stopped';
  /** Close the workspace (the route confirms first if the agent is mid-task). */
  onCloseFolder$?: QRL<() => void>;
  /** This project's permission mode (ask = default; auto = ordinary project
   *  work runs unasked, every decision still recorded). */
  permissionMode?: 'ask' | 'auto' | 'all';
  /** Switch this project's permission mode (remembered for the folder). */
  onSetPermissionMode$?: QRL<(mode: 'ask' | 'auto' | 'all') => void>;
  /** Installed agent supports Auto (v0.2.0+); false disables the Auto
   *  button and points at the update card in Settings > Components. */
  autoPermissionsSupported?: boolean;
  /** Build is installed - the workspace slot only exists then. */
  buildInstalled?: boolean;
  /** Recent workspaces, most-recent-first, for the slot's menu. */
  recentFolders?: string[];
  /** Open a workspace (from the recents menu). */
  onOpenFolder$?: QRL<(path: string) => void>;
  /** Open the folder picker. */
  onBrowseFolder$?: QRL<() => void>;
  /** Open the Conversations drawer (pick a conversation back up). */
  onOpenConversations$?: QRL<() => void>;
}

/** Home-shortened path for the slot ("~/Projects/Website"). */
function displayPath(p: string): string {
  return p.replace(/^\/(home|Users)\/[^/]+/, '~');
}

/** The project's leaf name - how people think of it ("Website"). */
function leafName(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() ?? p;
}

/** The agent rail's metal gradient - ties this menu to the same family. */
const METAL_H = "linear-gradient(90deg, #cfd8dc 0%, #59c9ff 45%, #7e99a6 100%)";

const PERMISSION_HINTS: Record<'ask' | 'auto' | 'all', string> = {
  ask: 'Your AI asks before every command and file change.',
  auto: 'Ordinary work in this folder runs without asking; risky, irreversible, or outside-the-folder actions ask.',
  all: 'Nothing asks. Every action is still written to your records.',
};

export default component$<AppHeaderProps>(
  ({
    handleNewQuestion$,
    handleModelsClick$,
    currentModel,
    isModelLoading = false,
    modelTooBig = false,
    modelIssue = "",
    showModelWidget = false,
    folderPath = null,
    folderStatus,
    onCloseFolder$,
    permissionMode = 'ask',
    onSetPermissionMode$,
    autoPermissionsSupported = true,
    buildInstalled = false,
    recentFolders = [],
    onOpenFolder$,
    onBrowseFolder$,
    onOpenConversations$,
  }) => {
    const nav = useNavigate();
    const { theme } = useContext(ThemeContext);

    // Controls visibility of the custom dropdown menu
    const menuOpen = useSignal(false);
    // The workspace slot's recents menu.
    const folderMenuOpen = useSignal(false);
    const workspaceMenuOpen = useSignal(false);
    // Skills the open project folder carries itself (.claude/.agents/.grok
    // skills dirs) - shown on the pill so a repo's own know-how is visible.
    const projectSkills = useSignal<string[]>([]);
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(async ({ track }) => {
      const p = track(() => folderPath);
      if (!p) {
        projectSkills.value = [];
        return;
      }
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        projectSkills.value = await invoke<string[]>('skills_in_folder', { path: p });
      } catch {
        projectSkills.value = [];
      }
    });
    // The project-memory modal lives at the layout root (route stacking
    // contexts would bury it here) - this signal opens it.
    const memoryFolder = useContext(ProjectMemoryContext);

    // Optional online-usage ticker (Settings -> Your Flowsta Account, OFF by
    // default). Polls only while enabled; hides itself signed-out or on free.
    const usageTicker = useSignal<UsageSummary | null>(null);
    const usageTickerOn = useSignal(false);

    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ cleanup }) => {
      let timer: ReturnType<typeof setTimeout> | null = null;
      // On by default now, so pace the poll by what it finds: a plan is
      // read every 90 s; nothing (signed out costs no request at all - the
      // backend returns before any call; free tier costs one) is re-read
      // every 30 min, and again on the next toggle or sign-in.
      const schedule = (ms: number) => {
        if (timer) clearTimeout(timer);
        timer = setTimeout(poll, ms);
      };
      const poll = () =>
        fetchUsage().then((u) => {
          usageTicker.value = u;
          if (usageTickerOn.value) schedule(u ? 90_000 : 30 * 60_000);
        });
      const apply = () => {
        usageTickerOn.value = usageTickerEnabled();
        if (timer) {
          clearTimeout(timer);
          timer = null;
        }
        if (usageTickerOn.value) {
          poll();
        } else {
          usageTicker.value = null;
        }
      };
      apply();
      window.addEventListener("usagePrefsChanged", apply);
      cleanup(() => {
        window.removeEventListener("usagePrefsChanged", apply);
        if (timer) clearTimeout(timer);
      });
    });
    // Install-on-demand: the projects surface shows for EVERYONE; the
    // agent add-on downloads from here. The download runs in the Rust
    // runtime - navigating away never interrupts it.
    const buildReady = useSignal(buildInstalled);
    const buildDownloading = useSignal(false);
    const buildProgress = useSignal(0);
    const buildError = useSignal<string | null>(null);
  const buildUpdateAvailable = useSignal(false);
  const buildInstalledVersion = useSignal<string | null>(null);
  const buildPinnedVersion = useSignal<string>("");

    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(async ({ cleanup }) => {
      const { listen } = await import("@tauri-apps/api/event");
      const unProgress = await listen<any>("model-download-progress", (e) => {
        const f = e.payload?.filename;
        if (typeof f === "string" && f.startsWith("your-own-ai-build-")) {
          buildDownloading.value = true;
          buildProgress.value = Math.round(e.payload?.percent ?? 0);
        }
      });
      const unDone = await listen<any>("build-install-done", (e) => {
        buildDownloading.value = false;
        buildReady.value = true;
        buildError.value = null;
        const path = e.payload?.path;
        if (typeof path === "string" && path) {
          try {
            localStorage.setItem("build-binary-path", path);
          } catch {
            /* resolver falls back */
          }
        }
      });
      const unFail = await listen<any>("build-install-failed", (e) => {
        buildDownloading.value = false;
        buildError.value = String(e.payload?.error ?? "download failed");
      });
      cleanup(() => {
        unProgress();
        unDone();
        unFail();
      });
    });

    const setTheme = $((t: AppTheme) => {
      theme.value = t;
      menuOpen.value = false;
    });

    const handleClick = $(() => {
      handleNewQuestion$();
    });

    const closeMenu = $(() => {
      menuOpen.value = false;
    });

    const toggleMenu = $(() => {
      menuOpen.value = !menuOpen.value;
    });

    return (
      <>
      <section
        class="relative z-30 bg-[var(--bg-header-footer)]"
      >
        <div class="px-4 py-3 sm:py-4 flex items-center justify-between">
          <div class="flex items-center flex-shrink-0">
            <button
              type="button"
              onClick$={handleClick}
              class="focus:outline-none inline-block cursor-pointer bg-transparent border-none p-0"
            >
              <Logo theme={theme.value} />
            </button>
          </div>

          <div class="flex items-center gap-4">
            {/* Online-usage ticker - opt-in, paid plans only */}
            {usageTickerOn.value && usageTicker.value && (
              <a
                href="/settings/"
                title="Online usage this month - allowance resets on the 1st. Click for details."
                class="text-xs text-[var(--text-secondary)] hidden lg:flex items-center gap-2 px-3 h-9 bg-[var(--bg-dropdown)] rounded-full border border-[var(--border-subtle)] hover:text-[var(--text-primary)] hover:border-[#71717a] transition-colors"
              >
                <span class="w-2 h-2 rounded-full bg-sky-500" />
                {formatUsage(usageTicker.value)}
              </a>
            )}
            {/* Current Model Badge with Status Indicator */}
            {showModelWidget && currentModel && (
              <span
                class="text-xs text-[var(--text-secondary)] hidden md:flex items-center gap-2 px-3 h-9 bg-[var(--bg-dropdown)] rounded-full border border-[var(--border-subtle)]"
                title="The model loaded on this device right now"
              >
                <span
                  class={`w-2 h-2 rounded-full ${
                    modelTooBig
                      ? "bg-red-500"
                      : isModelLoading
                        ? "bg-orange-500 animate-pulse"
                        : "bg-green-500"
                  }`}
                />
                {currentModel.replace(".gguf", "").replace(/-/g, " ")}
                {modelTooBig && (
                  <span class="text-red-500 font-medium">· {modelIssue || "too big"}</span>
                )}
              </span>
            )}

            {/* THE WORKSPACE SLOT - always present once Build is installed.
                Open: status dot + path (the mode is unmistakable).
                Closed: "Open a folder" with recents. Chatters (no Build)
                never see this. */}
            {buildInstalled && folderPath && (
              <span
                class="relative text-xs text-[var(--text-secondary)] flex items-center gap-2 px-3 h-9 bg-[var(--bg-dropdown)] rounded-full border border-[var(--border-subtle)] cursor-pointer"
                onClick$={() => (workspaceMenuOpen.value = !workspaceMenuOpen.value)}
                title={
                  folderStatus === 'starting'
                    ? `Getting this project ready - your AI is connecting to ${folderPath}. Ready in a few seconds.`
                    : folderStatus === 'stopped'
                      ? `The project's helper stopped - reopen ${folderPath} to keep working.`
                      : folderStatus === 'working'
                        ? `Your AI is working in ${folderPath} right now.`
                        : `This conversation's project: ${folderPath}. Your AI can read and change files here.`
                }
              >
                <span
                  class={`w-2 h-2 rounded-full shrink-0 ${
                    folderStatus === 'stopped'
                      ? 'bg-red-500'
                      : folderStatus === 'starting'
                        ? 'bg-orange-500 animate-pulse'
                        : folderStatus === 'working'
                          ? 'bg-green-500 animate-pulse'
                          : 'bg-green-500'
                  }`}
                />
                {/* Full path when there's room, keeping the leaf end when
                    there isn't (rtl clip = ellipsis at the start). */}
                <span
                  dir="rtl"
                  class="truncate max-w-[130px] sm:max-w-[240px] xl:max-w-[420px]"
                >
                  {'‎' + displayPath(folderPath)}
                </span>
                {permissionMode !== 'ask' && (
                  <span
                    class="shrink-0 rounded-full border border-[var(--border-subtle)] px-1.5 text-[10px] uppercase tracking-wide text-[var(--text-muted)]"
                    title={
                      permissionMode === 'all'
                        ? 'Approve everything: nothing asks; every action is in your records'
                        : 'Auto permissions: ordinary work inside this folder runs without asking; every decision is in your records'
                    }
                  >
                    {permissionMode === 'all' ? 'all' : 'auto'}
                  </span>
                )}
                {projectSkills.value.length > 0 && (
                  <span
                    class="shrink-0 rounded-full border border-[var(--border-subtle)] px-1.5 text-[10px] text-[var(--text-secondary)]"
                    title={`This project brings its own skills: ${projectSkills.value.join(', ')}`}
                  >
                    {projectSkills.value.length} skill{projectSkills.value.length === 1 ? '' : 's'}
                  </span>
                )}
                {folderStatus === 'stopped' && (
                  <span class="text-red-500 font-medium shrink-0">stopped</span>
                )}
                {onCloseFolder$ && (
                  <button
                    type="button"
                    onClick$={(e) => {
                      e.stopPropagation();
                      onCloseFolder$();
                    }}
                    title="Close this project"
                    class="ml-0.5 shrink-0 text-[var(--text-muted)] hover:text-[var(--text-primary)] leading-none"
                  >
                    &times;
                  </button>
                )}
                {workspaceMenuOpen.value && (
                  <>
                    <span
                      class="fixed inset-0 z-[45] cursor-default"
                      onClick$={(e) => {
                        e.stopPropagation();
                        workspaceMenuOpen.value = false;
                      }}
                    />
                    <span
                      class="absolute right-0 top-full mt-2 w-80 rounded-2xl bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] shadow-lg z-50 overflow-hidden block cursor-default text-left"
                      onClick$={(e) => e.stopPropagation()}
                    >
                      {/* Identity: the project as a thing - leaf name first,
                          path second, status in words (the chip's dot alone
                          explains nothing). */}
                      <span class="block px-4 pt-3.5 pb-3">
                        <span class="flex items-center gap-2.5">
                          <LuFolderOpen class="h-5 w-5 shrink-0 opacity-70 text-[var(--text-secondary)]" />
                          <span class="block min-w-0 flex-1">
                            <span class="block truncate text-sm font-semibold text-[var(--text-primary)]">
                              {leafName(folderPath)}
                            </span>
                            <span dir="rtl" class="block truncate text-[11px] text-[var(--text-muted)]">
                              {'\u200e' + displayPath(folderPath)}
                            </span>
                          </span>
                        </span>
                        <span class="mt-2 flex items-center gap-2 text-xs text-[var(--text-secondary)]">
                          <span
                            class={`w-2 h-2 rounded-full shrink-0 ${
                              folderStatus === 'stopped'
                                ? 'bg-red-500'
                                : folderStatus === 'starting'
                                  ? 'bg-orange-500 animate-pulse'
                                  : folderStatus === 'working'
                                    ? 'bg-green-500 animate-pulse'
                                    : 'bg-green-500'
                            }`}
                          />
                          <span>
                            {folderStatus === 'stopped'
                              ? 'Stopped - reopen to keep working'
                              : folderStatus === 'starting'
                                ? 'Starting up - ready in a few seconds'
                                : folderStatus === 'working'
                                  ? 'Working in this folder right now'
                                  : 'Ready - your AI can read and change files here'}
                          </span>
                        </span>
                        {folderStatus === 'stopped' && onOpenFolder$ && (
                          <LiquidMetalButton
                            class="mt-2.5 flex w-full items-center justify-center gap-1.5 px-3 py-1.5 text-xs"
                            onClick$={() => {
                              workspaceMenuOpen.value = false;
                              onOpenFolder$(folderPath);
                            }}
                          >
                            <LuRotateCw class="h-3.5 w-3.5" />
                            Reopen project
                          </LiquidMetalButton>
                        )}
                      </span>
                      <span
                        class="block h-[2px] opacity-60"
                        style={{ background: METAL_H }}
                      />
                      {onSetPermissionMode$ && (
                        <span class="block px-4 py-3 border-b border-[var(--border-subtle)]">
                          <span class="block text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)] mb-1.5">
                            Permissions
                          </span>
                          {autoPermissionsSupported ? (
                            <>
                              <span class="flex rounded-full border border-[var(--border-subtle)] overflow-hidden">
                                {(['ask', 'auto', 'all'] as const).map((mode, i) => (
                                  <button
                                    key={mode}
                                    type="button"
                                    onClick$={() => onSetPermissionMode$(mode)}
                                    class={`flex-1 py-1.5 text-xs ${i > 0 ? 'border-l border-[var(--border-subtle)]' : ''} ${
                                      permissionMode === mode
                                        ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)] font-medium'
                                        : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)] active:bg-[var(--bg-dropdown-hover)] cursor-pointer'
                                    }`}
                                  >
                                    {mode === 'ask' ? 'Ask' : mode === 'auto' ? 'Auto' : 'Everything'}
                                  </button>
                                ))}
                              </span>
                              {/* The ACTIVE mode's one-liner, always visible -
                                  this used to hide in hover tooltips. */}
                              <span class="block mt-1.5 text-[11px] leading-snug text-[var(--text-muted)]">
                                {PERMISSION_HINTS[permissionMode]}
                              </span>
                            </>
                          ) : (
                            <span class="block text-[11px] leading-snug text-[var(--text-muted)]">
                              Ask is on. Auto needs a newer Your Own AI Build -
                              update it in Settings &gt; Components, then reopen
                              the project.
                            </span>
                          )}
                        </span>
                      )}
                      <span class="grid grid-cols-2 gap-2 p-3">
                        <button
                          type="button"
                          onClick$={() => {
                            workspaceMenuOpen.value = false;
                            memoryFolder.value = folderPath;
                          }}
                          class={`flex flex-col items-center justify-center gap-1.5 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-3.5 text-xs text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] active:scale-[0.98] cursor-pointer ${onCloseFolder$ ? '' : 'col-span-2'}`}
                        >
                          <LuBrain class="h-5 w-5 opacity-70" />
                          Project notes
                        </button>
                        {onCloseFolder$ && (
                          <button
                            type="button"
                            onClick$={() => {
                              workspaceMenuOpen.value = false;
                              onCloseFolder$();
                            }}
                            class="flex flex-col items-center justify-center gap-1.5 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-3.5 text-xs text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] hover:text-red-500 hover:border-red-500/40 active:scale-[0.98] cursor-pointer"
                          >
                            <LuX class="h-5 w-5 opacity-70" />
                            Close project
                          </button>
                        )}
                      </span>
                    </span>
                  </>
                )}
              </span>
            )}
            {!folderPath && (
              <span class="relative">
                <LiquidMetalButton
                  onClick$={async () => {
                    folderMenuOpen.value = !folderMenuOpen.value;
                    if (folderMenuOpen.value) {
                      try {
                        const { invoke } = await import("@tauri-apps/api/core");
                        const s = (await invoke("build_install_status")) as {
                          installed: boolean;
                          downloading: boolean;
                          error: string | null;
                          update_available: boolean;
                          installed_version: string | null;
                          pinned_version: string;
                        };
                        buildReady.value = s.installed || buildInstalled;
                        buildDownloading.value = s.downloading;
                        buildError.value = s.error;
                        buildUpdateAvailable.value = s.installed && s.update_available;
                        buildInstalledVersion.value = s.installed_version;
                        buildPinnedVersion.value = s.pinned_version.replace(/^v/, "");
                      } catch {
                        buildReady.value = buildInstalled;
                      }
                    }
                  }}
                  variant="secondary"
                  class="flex items-center justify-center w-9 h-9 cursor-pointer relative"
                  title={
                    buildDownloading.value
                      ? `Downloading Your Own AI Build - ${buildProgress.value}%`
                      : "Projects - your AI working in a folder with you"
                  }
                >
                  <LuFolderOpen class="h-[18px] w-[18px]" />
                  {buildDownloading.value && (
                    <span class="absolute -top-0.5 -right-0.5 w-2.5 h-2.5 rounded-full bg-orange-500 animate-pulse" />
                  )}
                </LiquidMetalButton>
                {folderMenuOpen.value && (
                  <>
                    <span
                      class="fixed inset-0 z-[45]"
                      onClick$={() => (folderMenuOpen.value = false)}
                    />
                    <span class="absolute right-0 top-full mt-2 w-[26rem] max-w-[92vw] rounded-2xl bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] p-3 shadow-lg z-50 block">
                      {/* The picker: one big deliberate act, recents beside it. */}
                      <span class="grid grid-cols-2 gap-3 block">
                        <button
                          type="button"
                          disabled={!buildReady.value}
                          title={
                            buildReady.value
                              ? "Pick any folder on your computer"
                              : "Download Your Own AI Build below to activate projects"
                          }
                          onClick$={() => {
                            folderMenuOpen.value = false;
                            onBrowseFolder$?.();
                          }}
                          class={`flex flex-col items-center justify-center gap-2 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-6 text-sm text-[var(--text-dropdown)] ${
                            buildReady.value
                              ? "hover:bg-[var(--bg-dropdown-hover)] cursor-pointer"
                              : "opacity-40 cursor-default"
                          }`}
                        >
                          <LuFolderOpen class="h-8 w-8 opacity-80" />
                          <span class="text-center leading-snug">
                            Choose the
                            <br />
                            project's folder
                          </span>
                        </button>
                        <span class="flex flex-col min-w-0 block">
                          <span class="block px-1 pb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)]">
                            Recent projects
                          </span>
                          {buildReady.value && recentFolders.length > 0 ? (
                            recentFolders.slice(0, 5).map((p) => (
                              <button
                                key={p}
                                type="button"
                                onClick$={() => {
                                  folderMenuOpen.value = false;
                                  onOpenFolder$?.(p);
                                }}
                                title={p}
                                class="flex w-full items-center gap-2 px-2 py-1.5 rounded-lg text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] min-w-0"
                              >
                                <LuFolderOpen class="h-3.5 w-3.5 shrink-0 opacity-50" />
                                <span dir="rtl" class="truncate">
                                  {'‎' + displayPath(p)}
                                </span>
                              </button>
                            ))
                          ) : (
                            <span class="block px-2 py-1.5 text-xs text-[var(--text-muted)]">
                              {buildReady.value
                                ? "Projects you open will gather here."
                                : "Your recent projects will appear here."}
                            </span>
                          )}
                        </span>
                      </span>

                      {/* Installed but behind the version this app ships:
                          the update lives HERE too - the folder menu is where
                          project users look, same alert treatment as the
                          original install message. Same downloader; it
                          replaces the old binary with the pinned release. */}
                      {buildReady.value && buildUpdateAvailable.value && !buildDownloading.value && (
                        <span class="block mt-3">
                          <Callout intent="info" title="Update available">
                            A newer Your Own AI Build is ready
                            {buildPinnedVersion.value ? ` - version ${buildPinnedVersion.value}` : ''}
                            {buildInstalledVersion.value ? ` (you have ${buildInstalledVersion.value})` : ''}.
                            It brings auto permissions and the newest agent
                            improvements. Update below.
                          </Callout>
                          <span class="block mt-2">
                            {buildError.value && (
                              <span class="block px-1 pb-1 text-xs text-red-500 dark:text-red-400">
                                The download hit a problem: {buildError.value}
                              </span>
                            )}
                            <LiquidMetalButton
                              class="w-full px-4 py-2 text-sm"
                              onClick$={async () => {
                                buildError.value = null;
                                buildDownloading.value = true;
                                buildProgress.value = 0;
                                buildUpdateAvailable.value = false;
                                try {
                                  const { invoke } = await import("@tauri-apps/api/core");
                                  invoke("download_build_agent").catch(() => {
                                    /* failure arrives via its event */
                                  });
                                } catch {
                                  buildDownloading.value = false;
                                }
                              }}
                            >
                              {buildError.value
                                ? "Try the update again"
                                : `Update Your Own AI Build${buildPinnedVersion.value ? ` to ${buildPinnedVersion.value}` : ''}`}
                            </LiquidMetalButton>
                          </span>
                        </span>
                      )}
                      {buildReady.value && buildDownloading.value && (
                        <span class="block mt-3 px-1">
                          <span class="flex items-center justify-between text-xs text-[var(--text-secondary)] mb-1">
                            <span>Updating Your Own AI Build..</span>
                            <span>{buildProgress.value}%</span>
                          </span>
                          <span class="block h-1.5 rounded-full bg-[var(--border-subtle)] overflow-hidden">
                            <span
                              class="block h-full rounded-full bg-[var(--text-link)] transition-all"
                              style={{ width: `${buildProgress.value}%` }}
                            />
                          </span>
                          <span class="block mt-1 text-[11px] text-[var(--text-muted)]">
                            Keep using the app - this continues in the
                            background.
                          </span>
                        </span>
                      )}
                      {/* Not installed yet: what projects are + the one step. */}
                      {!buildReady.value && (
                        <span class="block mt-3">
                          <Callout intent="info" title="Projects">
                            Open a project folder for agentic coding - Read
                            files, make edits, and run commands, always with
                            your permission. Every step recorded in your
                            private transcripts. Projects need Your Own AI
                            Build, a free add-on. Download below.
                          </Callout>
                          {buildDownloading.value ? (
                            <span class="block mt-2 px-1">
                              <span class="flex items-center justify-between text-xs text-[var(--text-secondary)] mb-1">
                                <span>Downloading Your Own AI Build..</span>
                                <span>{buildProgress.value}%</span>
                              </span>
                              <span class="block h-1.5 rounded-full bg-[var(--border-subtle)] overflow-hidden">
                                <span
                                  class="block h-full rounded-full bg-[var(--text-link)] transition-all"
                                  style={{ width: `${buildProgress.value}%` }}
                                />
                              </span>
                              <span class="block mt-1 text-[11px] text-[var(--text-muted)]">
                                Keep using the app - this continues in the
                                background.
                              </span>
                            </span>
                          ) : (
                            <span class="block mt-2">
                              {buildError.value && (
                                <span class="block px-1 pb-1 text-xs text-red-500 dark:text-red-400">
                                  The download hit a problem: {buildError.value}
                                </span>
                              )}
                              <LiquidMetalButton
                                class="w-full px-4 py-2 text-sm"
                                onClick$={async () => {
                                  buildError.value = null;
                                  buildDownloading.value = true;
                                  buildProgress.value = 0;
                                  try {
                                    const { invoke } = await import("@tauri-apps/api/core");
                                    invoke("download_build_agent").catch(() => {
                                      /* failure arrives via its event */
                                    });
                                  } catch {
                                    buildDownloading.value = false;
                                  }
                                }}
                              >
                                {buildError.value
                                  ? "Try the download again"
                                  : "Download Your Own AI Build (about 50 MB)"}
                              </LiquidMetalButton>
                            </span>
                          )}
                        </span>
                      )}
                    </span>
                  </>
                )}
              </span>
            )}

            {/* Conversations - pick any conversation back up. */}
            {onOpenConversations$ && (
              <LiquidMetalButton
                onClick$={onOpenConversations$}
                variant="secondary"
                class="flex items-center justify-center w-9 h-9 cursor-pointer"
                title="Conversations - pick up any conversation where you left off"
              >
                <LuMessagesSquare class="h-[18px] w-[18px]" />
              </LiquidMetalButton>
            )}

            {/* New Chat Button — Liquid Metal shader */}
            <LiquidMetalButton
              onClick$={handleClick}
              class="flex items-center gap-2.5 px-4 sm:px-5 h-9 text-[0.9375rem] cursor-pointer"
              title="Start a fresh conversation"
            >
              <svg
                width="18"
                height="18"
                viewBox="0 0 16 16"
                fill="currentColor"
                aria-hidden="true"
              >
                <path d="M2.678 11.894a1 1 0 0 1 .287.801 10.97 10.97 0 0 1-.398 2c1.395-.323 2.247-.697 2.634-.893a1 1 0 0 1 .71-.074A8.06 8.06 0 0 0 8 14c3.996 0 7-2.807 7-6s-3.004-6-7-6-7 2.808-7 6c0 1.468.617 2.83 1.678 3.894zm-.493 3.905a21.682 21.682 0 0 1-.713.129c-.2.032-.352-.176-.273-.362a9.68 9.68 0 0 0 .244-.637l.003-.01c.248-.72.45-1.548.524-2.319C.743 11.37 0 9.76 0 8c0-3.866 3.582-7 8-7s8 3.134 8 7-3.582 7-8 7a9.06 9.06 0 0 1-2.347-.306c-.52.263-1.639.742-3.468 1.105z" />
              </svg>
              <span class="hidden sm:inline">New</span>
            </LiquidMetalButton>

            {/* Settings Menu — custom dropdown replacing @headlessui/react Menu */}
            <div class="relative inline-block text-left">
              <LiquidMetalButton
                onClick$={toggleMenu}
                class="flex items-center justify-center h-9 w-9 cursor-pointer"
                title="Menu - Your AIs, memory, settings, and theme"
              >
                <LuSettings2 class="w-[18px] h-[18px]" />
              </LiquidMetalButton>

              {/*
                Backdrop overlay — closes menu when clicking outside.
                Only rendered when menu is open.
              */}
              {menuOpen.value && (
                <div
                  class="fixed inset-0 z-40"
                  onClick$={closeMenu}
                  aria-hidden="true"
                />
              )}

              {/*
                Dropdown panel.
                CSS transition classes handle the open/close animation.
              */}
              <div
                class={[
                  "absolute right-0 mt-2 w-56 origin-top-right divide-y divide-[var(--border-divider)] rounded-2xl bg-[var(--bg-card)] shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none z-50 overflow-hidden",
                  // a touch of word/letter spacing so multi-word items breathe
                  "tracking-wide [word-spacing:0.12em]",
                  "transition-all duration-100 ease-out",
                  menuOpen.value
                    ? "opacity-100 scale-100 pointer-events-auto"
                    : "opacity-0 scale-95 pointer-events-none",
                ]}
                role="menu"
              >
                {/* Theme Selector */}
                <div class="py-1">
                  <div class="px-3 py-2 text-xs font-semibold text-[var(--text-secondary)]">
                    Theme
                  </div>
                  <button
                    onClick$={() => setTheme("light")}
                    class="group flex w-full items-center justify-between pl-3 pr-2 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                    role="menuitem"
                  >
                    <div class="flex items-center">
                      <LuSun class="mr-2 h-5 w-5" aria-hidden="true" />
                      Light
                    </div>
                    {theme.value === "light" && (
                      <LuCheck class="w-4 h-4 text-[var(--text-link)]" />
                    )}
                  </button>
                  <button
                    onClick$={() => setTheme("dark")}
                    class="group flex w-full items-center justify-between pl-3 pr-2 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                    role="menuitem"
                  >
                    <div class="flex items-center">
                      <LuMoon
                        class="mr-2 h-5 w-5"
                        aria-hidden="true"
                      />
                      Dark
                    </div>
                    {theme.value === "dark" && (
                      <LuCheck class="w-4 h-4 text-[var(--text-link)]" />
                    )}
                  </button>
                </div>

                {/* Navigation — grouped: identity/memory · models · settings.
                    Each group is its own div so the menu's divide-y draws a
                    line + spacing between them (matching the Theme divider). */}
                <div class="py-1">
                  <button
                    onClick$={async () => {
                      menuOpen.value = false;
                      await nav("/your-ais");
                    }}
                    class="group flex w-full items-center pl-3 pr-2 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                    role="menuitem"
                  >
                    <LuBot class="mr-2 h-5 w-5" aria-hidden="true" />
                    Your AIs
                  </button>
                  <button
                    onClick$={async () => {
                      menuOpen.value = false;
                      await nav("/your-memory/");
                    }}
                    class="group flex w-full items-center pl-3 pr-2 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                    role="menuitem"
                  >
                    <LuBrain class="mr-2 h-5 w-5" aria-hidden="true" />
                    Your Memory
                  </button>
                </div>
                <div class="py-1">
                  <button
                    onClick$={() => {
                      menuOpen.value = false;
                      handleModelsClick$();
                    }}
                    class="group flex w-full items-center pl-3 pr-2 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                    role="menuitem"
                  >
                    <LuDownload
                      class="mr-2 h-5 w-5"
                      aria-hidden="true"
                    />
                    Offline Models
                  </button>
                  <button
                    onClick$={async () => {
                      menuOpen.value = false;
                      await nav("/online-models");
                    }}
                    class="group flex w-full items-center pl-3 pr-2 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                    role="menuitem"
                  >
                    <LuCloud class="mr-2 h-5 w-5" aria-hidden="true" />
                    Online Models
                  </button>
                </div>
                <div class="py-1">
                  <button
                    onClick$={async () => {
                      menuOpen.value = false;
                      await nav("/add-ons");
                    }}
                    class="group flex w-full items-center pl-3 pr-2 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] transition-colors"
                    role="menuitem"
                  >
                    <LuPuzzle class="mr-2 h-5 w-5" aria-hidden="true" />
                    Add-ons
                  </button>
                </div>
                <div class="py-1">
                  <button
                    onClick$={async () => {
                      menuOpen.value = false;
                      await nav("/settings");
                    }}
                    class="group flex w-full items-center pl-3 pr-2 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                    role="menuitem"
                  >
                    <LuSettings
                      class="mr-2 h-5 w-5"
                      aria-hidden="true"
                    />
                    Settings
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>

      </section>
      </>
    );
  }
);
