/**
 * App Header - Qwik version (migrated from React)
 * Simplified for local-first app (no authentication)
 */

import {
  component$,
  useContext,
  useSignal,
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
} from "@qwikest/icons/lucide";

import { ThemeContext, type AppTheme } from "../routes/layout";

import LiquidMetalButton from "./LiquidMetalButton";
import { WorkspaceMemoryModal } from "./WorkspaceMemoryModal";
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
  showModelWidget?: boolean;
  /** The app-wide workspace folder - null = none open. */
  folderPath?: string | null;
  /** Agent session status while a folder is open. */
  folderStatus?: 'starting' | 'ready' | 'working' | 'stopped';
  /** Close the workspace (the route confirms first if the agent is mid-task). */
  onCloseFolder$?: QRL<() => void>;
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

export default component$<AppHeaderProps>(
  ({
    handleNewQuestion$,
    handleModelsClick$,
    currentModel,
    isModelLoading = false,
    modelTooBig = false,
    showModelWidget = false,
    folderPath = null,
    folderStatus,
    onCloseFolder$,
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
    // Non-null = the workspace-memory modal is open for this folder.
    const memoryFolder = useSignal<string | null>(null);

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
                    <span class="absolute right-0 top-full mt-2 w-72 rounded-2xl bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] py-1 shadow-lg z-50 overflow-hidden block cursor-default text-left">
                      <span class="block px-3 py-2 text-xs text-[var(--text-muted)] break-all border-b border-[var(--border-subtle)]">
                        {folderPath}
                      </span>
                      <button
                        type="button"
                        onClick$={(e) => {
                          e.stopPropagation();
                          workspaceMenuOpen.value = false;
                          memoryFolder.value = folderPath;
                        }}
                        class="flex w-full items-center gap-2 px-3 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                      >
                        <LuBrain class="h-4 w-4 shrink-0 opacity-70" />
                        Project memory
                      </button>
                      {onCloseFolder$ && (
                        <button
                          type="button"
                          onClick$={(e) => {
                            e.stopPropagation();
                            workspaceMenuOpen.value = false;
                            onCloseFolder$();
                          }}
                          class="flex w-full items-center gap-2 px-3 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                        >
                          <span class="w-4 text-center leading-none">&times;</span>
                          Close project
                        </button>
                      )}
                    </span>
                  </>
                )}
              </span>
            )}
            {buildInstalled && !folderPath && (
              <span class="relative">
                <LiquidMetalButton
                  onClick$={() => (folderMenuOpen.value = !folderMenuOpen.value)}
                  variant="secondary"
                  class="flex items-center justify-center w-9 h-9 cursor-pointer"
                  title="Open a project - your AI can work inside it"
                >
                  <LuFolderOpen class="h-[18px] w-[18px]" />
                </LiquidMetalButton>
                {folderMenuOpen.value && (
                  <>
                    <span
                      class="fixed inset-0 z-[45]"
                      onClick$={() => (folderMenuOpen.value = false)}
                    />
                    <span class="absolute right-0 top-full mt-2 w-64 rounded-2xl bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] py-1 shadow-lg z-50 overflow-hidden block">
                      {recentFolders.map((p) => (
                        <button
                          key={p}
                          type="button"
                          onClick$={() => {
                            folderMenuOpen.value = false;
                            onOpenFolder$?.(p);
                          }}
                          title={p}
                          class="flex w-full items-center gap-2 px-3 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                        >
                          <LuFolderOpen class="h-4 w-4 shrink-0 opacity-60" />
                          <span dir="rtl" class="truncate">
                            {'‎' + displayPath(p)}
                          </span>
                        </button>
                      ))}
                      {recentFolders.length > 0 && (
                        <span class="block border-t border-[var(--border-subtle)] my-1" />
                      )}
                      <button
                        type="button"
                        onClick$={() => {
                          folderMenuOpen.value = false;
                          onBrowseFolder$?.();
                        }}
                        class="flex w-full items-center gap-2 px-3 py-2 text-sm text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)]"
                      >
                        <LuFolderOpen class="h-4 w-4 shrink-0" />
                        Choose the project's folder..
                      </button>
                    </span>
                  </>
                )}
              </span>
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
                  <span class="text-red-500 font-medium">· too big</span>
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
      {/* Outside the header's stacking context (relative z-30) - inside it,
          the modal could never rise above page dropdowns. */}
      <WorkspaceMemoryModal folderPath={memoryFolder} />
      </>
    );
  }
);
