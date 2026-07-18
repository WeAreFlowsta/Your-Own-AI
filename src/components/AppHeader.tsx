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
  LuBrain,
  LuDownload,
  LuCloud,
} from "@qwikest/icons/lucide";

import { ThemeContext, type AppTheme } from "../routes/layout";

import LiquidMetalButton from "./LiquidMetalButton";
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
  /** Open folder (Build agent) for this conversation - null = none. */
  folderPath?: string | null;
  /** Agent session status while a folder is open. */
  folderStatus?: 'starting' | 'ready' | 'working' | 'stopped';
  /** Close the folder (the route confirms first if the agent is mid-task). */
  onCloseFolder$?: QRL<() => void>;
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
  }) => {
    const nav = useNavigate();
    const { theme } = useContext(ThemeContext);

    // Controls visibility of the custom dropdown menu
    const menuOpen = useSignal(false);

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
            {/* Folder chip - the durable "this conversation has hands in a
                folder" state. Same pill family as the model badge. */}
            {folderPath && (
              <span class="text-xs text-[var(--text-secondary)] flex items-center gap-2 px-3 py-1 bg-[var(--bg-dropdown)] rounded-full border border-[var(--border-subtle)]">
                <span
                  class={`w-2 h-2 rounded-full ${
                    folderStatus === 'stopped'
                      ? 'bg-red-500'
                      : folderStatus === 'starting'
                        ? 'bg-orange-500 animate-pulse'
                        : folderStatus === 'working'
                          ? 'bg-green-500 animate-pulse'
                          : 'bg-green-500'
                  }`}
                />
                <span class="max-w-[180px] truncate" title={folderPath}>
                  {folderPath.split('/').filter(Boolean).pop() || folderPath}
                </span>
                {folderStatus === 'stopped' && (
                  <span class="text-red-500 font-medium">· stopped</span>
                )}
                {onCloseFolder$ && (
                  <button
                    type="button"
                    onClick$={onCloseFolder$}
                    title="Close this folder"
                    class="ml-0.5 text-[var(--text-muted)] hover:text-[var(--text-primary)] leading-none"
                  >
                    &times;
                  </button>
                )}
              </span>
            )}

            {/* Current Model Badge with Status Indicator */}
            {showModelWidget && currentModel && (
              <span class="text-xs text-[var(--text-secondary)] hidden md:flex items-center gap-2 px-3 py-1 bg-[var(--bg-dropdown)] rounded-full border border-[var(--border-subtle)]">
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

            {/* New Chat Button — Liquid Metal shader */}
            <LiquidMetalButton
              onClick$={handleClick}
              class="flex items-center gap-2.5 px-4 sm:px-5 h-9 text-[0.9375rem] cursor-pointer"
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
    );
  }
);
