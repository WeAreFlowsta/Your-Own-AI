import {
  component$,
  useSignal,
  useContext,
  useVisibleTask$,
  Slot,
  $,
  type Signal,
  type QRL,
} from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import AppHeader from "../../components/AppHeader";
import packageJson from "../../../package.json";
import { ThemeContext } from "../layout";
import { LiquidMetalBorder } from "../../components/LiquidMetalBorder";
import FlowstaAccount from "../../components/FlowstaAccount";
import ComponentsSettings from "../../components/ComponentsSettings";
import EnginesSettings from "../../components/EnginesSettings";
import ExternalAccess from "../../components/ExternalAccess";
import ConfirmModal from "../../components/ConfirmModal";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import { helpTipsEnabled, setHelpTipsEnabled, resetHelpDismissals } from "../../utils/helpPrefs";
import {
  getRememberScope,
  setRememberScope,
  type RememberScope,
  type RememberSurface,
} from "../../utils/rememberText";
import { isMemoryPaused, setMemoryPaused } from "../../utils/memory";
import { LuChevronDown } from "@qwikest/icons/lucide";

/** One "which online model answers" row: label + recommended-or-override
    dropdown. Native <select> popups are GTK-themed on webkit (they ignore our
    light/dark vars), so this uses the same custom-dropdown pattern as the
    Offline Models sort control. */
const OnlineModelPicker = component$<{
  label: string;
  hint: string;
  recommended: string;
  storageKey: string;
  selected: Signal<string>;
  models: Signal<{ id: string; display_name: string }[]>;
}>((props) => {
  const open = useSignal(false);
  const currentLabel = props.selected.value
    ? (props.models.value.find((m) => m.id === props.selected.value)?.display_name ??
      props.selected.value.replace("online:", ""))
    : `Recommended (${props.recommended})`;
  return (
    <div class="flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-4 py-3 border-b border-[var(--border-subtle)] last:border-b-0">
      <div class="flex-1 min-w-0">
        <div class="text-sm font-medium text-[var(--text-primary)]">{props.label}</div>
        <div class="text-xs text-[var(--text-secondary)] mt-0.5">{props.hint}</div>
      </div>
      <div class="relative shrink-0">
        <button
          type="button"
          onClick$={() => { open.value = !open.value; }}
          class="flex items-center justify-between gap-2 min-w-[13rem] px-3 py-2 rounded-xl text-sm bg-[var(--bg-main)] text-[var(--text-primary)] border border-[var(--border-subtle)] hover:opacity-90 focus:outline-none"
        >
          <span class="truncate">{currentLabel}</span>
          <LuChevronDown class="w-4 h-4 text-[var(--text-muted)] flex-shrink-0" />
        </button>
        {open.value && (
          <>
            <div class="fixed inset-0 z-40" onClick$={() => { open.value = false; }} />
            <div class="absolute right-0 top-full mt-1 min-w-[13rem] max-h-64 overflow-y-auto z-50 rounded-lg bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] shadow-xl py-1">
              <button
                type="button"
                onClick$={() => {
                  props.selected.value = "";
                  localStorage.removeItem(props.storageKey);
                  open.value = false;
                }}
                class={`block w-full text-left px-3 py-1.5 text-sm hover:bg-[var(--bg-card)] transition-colors ${
                  !props.selected.value
                    ? "text-[var(--text-primary)] font-medium"
                    : "text-[var(--text-secondary)]"
                }`}
              >
                Recommended ({props.recommended})
              </button>
              {props.models.value.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  onClick$={() => {
                    props.selected.value = m.id;
                    localStorage.setItem(props.storageKey, m.id);
                    open.value = false;
                  }}
                  class={`block w-full text-left px-3 py-1.5 text-sm hover:bg-[var(--bg-card)] transition-colors ${
                    m.id === props.selected.value
                      ? "text-[var(--text-primary)] font-medium"
                      : "text-[var(--text-secondary)]"
                  }`}
                >
                  {m.display_name}
                </button>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
});

/** The page's section order — drives both the layout and the jump nav. */
const SECTIONS = [
  { id: "settings-account", label: "Flowsta Account" },
  { id: "settings-backups", label: "Backups & recovery" },
  { id: "settings-behavior", label: "AI behavior" },
  { id: "settings-memory", label: "Memory" },
  { id: "settings-routing", label: "Routing" },
  { id: "settings-components", label: "Components" },
  { id: "settings-engines", label: "Engines" },
  { id: "settings-external", label: "External access" },
  { id: "settings-appearance", label: "Appearance" },
  { id: "settings-reset", label: "Reset" },
];

/** One titled toggle row — the standard switch used across settings.
 *  The description is the Slot (some descriptions carry links/buttons). */
const SettingToggle = component$<{
  title: string;
  checked: Signal<boolean>;
  onToggle$: QRL<() => void>;
}>((p) => {
  const { theme } = useContext(ThemeContext);
  return (
    <div class="flex items-center justify-between gap-4">
      <div>
        <h3 class="text-lg font-semibold text-[var(--text-primary)]">{p.title}</h3>
        <p class="text-sm text-[var(--text-secondary)] mt-1">
          <Slot />
        </p>
      </div>
      <LiquidMetalBorder
        borderRadius="9999px"
        theme={theme.value}
        style={{
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          verticalAlign: "middle",
          lineHeight: "0",
        }}
      >
        <button
          onClick$={p.onToggle$}
          class="gradient-border-target relative inline-flex h-6 w-11 items-center rounded-full transition-colors"
          style={{
            backgroundColor: p.checked.value
              ? theme.value === "dark"
                ? "#16a34a"
                : "var(--bg-button-primary)"
              : "var(--border-subtle)",
            margin: "0",
            padding: "0",
            border: "none",
            verticalAlign: "middle",
          }}
          role="switch"
          aria-checked={p.checked.value}
        >
          <span
            class={`inline-block h-4 w-4 transform rounded-full shadow-md transition-transform ${
              p.checked.value ? "translate-x-6 bg-white" : "translate-x-1 bg-white"
            }`}
          />
        </button>
      </LiquidMetalBorder>
    </div>
  );
});

/** Where one "Remember this" surface saves to: that AI only, or the shared
 *  notes every AI can draw on. Same two-option grid as the Routing pickers. */
const RememberDestinationPicker = component$<{
  title: string;
  hint: string;
  surface: RememberSurface;
  scope: Signal<RememberScope>;
}>((props) => {
  const options: { id: RememberScope; label: string; hint: string }[] = [
    {
      id: "per-ai",
      label: "That AI only",
      hint: "Stays with the AI you were talking to - never surfaces anywhere else",
    },
    {
      id: "global",
      label: "All your AIs",
      hint: "Saved to your shared notes - any AI can draw on it",
    },
  ];
  return (
    <div>
      <h4 class="text-base font-semibold text-[var(--text-primary)]">{props.title}</h4>
      <p class="text-sm text-[var(--text-secondary)] mt-1 mb-3">{props.hint}</p>
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
        {options.map((opt) => (
          <button
            key={opt.id}
            onClick$={() => {
              props.scope.value = opt.id;
              setRememberScope(props.surface, opt.id);
            }}
            class={`text-left rounded-xl p-3 border transition-colors ${
              props.scope.value === opt.id
                ? "bg-[var(--bg-button-primary)] text-[var(--text-button-primary)] border-[var(--border-subtle)]"
                : "bg-[var(--bg-main)] text-[var(--text-primary)] border-[var(--border-subtle)] hover:opacity-90"
            }`}
            aria-pressed={props.scope.value === opt.id}
          >
            <div class="font-semibold text-sm">{opt.label}</div>
            <div
              class={`text-xs mt-1 ${
                props.scope.value === opt.id ? "" : "text-[var(--text-secondary)]"
              }`}
            >
              {opt.hint}
            </div>
          </button>
        ))}
      </div>
    </div>
  );
});

export default component$(() => {
  const nav = useNavigate();

  const showModelWidget = useSignal(false);
  const showHelpTips = useSignal(true);
  const allowAttachmentsOnline = useSignal(false);
  const groundDocumentsAuto = useSignal(false);
  const smartModeDetection = useSignal(true);
  // Same key the working box's brain icon toggles - one setting, two doors.
  // Only shown when the Build agent is actually installed.
  const agentShowThoughts = useSignal(false);
  const buildInstalled = useSignal(false);
  const currentModel = useSignal<string | null>(null);
  const rememberScopeSelection = useSignal<RememberScope>("per-ai");
  const rememberScopeReply = useSignal<RememberScope>("per-ai");
  // "Automatic learning" = NOT paused (the toggle reads positively; the
  // stored flag is the pause, shared with the Your Memory page).
  const memoryLearning = useSignal(true);
  const routingEagerness = useSignal<"privacy" | "balanced" | "freshness">("balanced");
  const routingLean = useSignal<"speed" | "balanced" | "quality">("balanced");
  // Per-slot online model overrides ("" = the router's recommended default).
  const onlineFresh = useSignal("");
  const onlineHardCode = useSignal("");
  const onlineHardGeneral = useSignal("");
  const onlineModels = useSignal<{ id: string; display_name: string }[]>([]);
  // May online routing options be OFFERED? (signed in + plan; starts true so
  // a slow check never hides controls from a paying user.)
  const onlineEntitled = useSignal(true);
  // The running build's version (tauri.conf.json, which CI enforces equals
  // the release tag). package.json is only the pre-hydration fallback.
  const appVersion = useSignal(packageJson.version);

  // Initialise from localStorage
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    // Init
    const savedWidget = localStorage.getItem("showModelWidget");
    showModelWidget.value =
      savedWidget === null ? false : savedWidget === "true";
    showHelpTips.value = helpTipsEnabled();
    allowAttachmentsOnline.value =
      localStorage.getItem("allowAttachmentsOnline") === "true";
    groundDocumentsAuto.value =
      localStorage.getItem("groundDocumentsAuto") === "true";
    smartModeDetection.value =
      localStorage.getItem("smartModeDetection") !== "false"; // default ON
    agentShowThoughts.value = localStorage.getItem("agent-show-thoughts") === "1";
    // Async: reveal the Build settings once the agent binary is confirmed.
    Promise.all([
      import("@tauri-apps/api/core"),
      import("../../hooks/useAgentSession"),
    ])
      .then(([{ invoke }, { resolveBinaryPath }]) =>
        invoke<boolean>("path_is_file", { path: resolveBinaryPath() }),
      )
      .then((present) => (buildInstalled.value = present))
      .catch(() => (buildInstalled.value = false));
    rememberScopeSelection.value = getRememberScope("selection");
    rememberScopeReply.value = getRememberScope("reply");
    memoryLearning.value = !isMemoryPaused();
    currentModel.value = localStorage.getItem("currentModel");
    const savedEagerness = localStorage.getItem("smartRoutingEagerness");
    if (savedEagerness === "privacy" || savedEagerness === "freshness") {
      routingEagerness.value = savedEagerness;
    }
    const savedLean = localStorage.getItem("routingOfflineLean");
    if (savedLean === "speed" || savedLean === "quality") {
      routingLean.value = savedLean;
    }
    onlineFresh.value = localStorage.getItem("routingOnlineFresh") || "";
    onlineHardCode.value = localStorage.getItem("routingOnlineHardCode") || "";
    onlineHardGeneral.value = localStorage.getItem("routingOnlineHardGeneral") || "";

    import("@tauri-apps/api/app")
      .then(({ getVersion }) => getVersion())
      .then((v) => { appVersion.value = v; })
      .catch(() => { /* keep package.json fallback */ });

    import("../../utils/entitlement")
      .then(({ getOnlineEntitlement }) => getOnlineEntitlement())
      .then(async (e) => {
        onlineEntitled.value = e.entitled;
        if (e.entitled) {
          // Populate the per-slot model pickers from the live catalog.
          const { invoke } = await import("@tauri-apps/api/core");
          const models =
            await invoke<{ id: string; display_name: string }[]>("list_online_models");
          onlineModels.value = models.map(({ id, display_name }) => ({ id, display_name }));
        }
      })
      .catch(() => { /* keep fail-open default */ });

    const handleStorageChange = () => {
      currentModel.value = localStorage.getItem("currentModel");
    };

    window.addEventListener("storage", handleStorageChange);
    window.addEventListener("focus", handleStorageChange);

    return () => {
      window.removeEventListener("storage", handleStorageChange);
      window.removeEventListener("focus", handleStorageChange);
    };
  });

  const toggleModelWidget = $(() => {
    showModelWidget.value = !showModelWidget.value;
    localStorage.setItem(
      "showModelWidget",
      showModelWidget.value.toString()
    );
    // Dispatch custom event so ChatPage can react immediately
    window.dispatchEvent(
      new CustomEvent("settingsChanged", {
        detail: { showModelWidget: showModelWidget.value },
      })
    );
  });

  const toggleHelpTips = $(() => {
    showHelpTips.value = !showHelpTips.value;
    setHelpTipsEnabled(showHelpTips.value);
  });

  const toggleAttachmentsOnline = $(() => {
    allowAttachmentsOnline.value = !allowAttachmentsOnline.value;
    localStorage.setItem(
      "allowAttachmentsOnline",
      allowAttachmentsOnline.value.toString()
    );
  });

  const toggleMemoryLearning = $(() => {
    memoryLearning.value = !memoryLearning.value;
    setMemoryPaused(!memoryLearning.value);
  });

  const toggleSmartMode = $(() => {
    smartModeDetection.value = !smartModeDetection.value;
    localStorage.setItem("smartModeDetection", smartModeDetection.value.toString());
  });

  const toggleAgentShowThoughts = $(() => {
    agentShowThoughts.value = !agentShowThoughts.value;
    localStorage.setItem("agent-show-thoughts", agentShowThoughts.value ? "1" : "0");
  });

  const toggleGroundDocuments = $(() => {
    groundDocumentsAuto.value = !groundDocumentsAuto.value;
    localStorage.setItem(
      "groundDocumentsAuto",
      groundDocumentsAuto.value.toString()
    );
  });

  const resetTips = $(() => {
    resetHelpDismissals();
    showHelpTips.value = true;
    setHelpTipsEnabled(true);
  });

  const setEagerness = $((value: "privacy" | "balanced" | "freshness") => {
    routingEagerness.value = value;
    localStorage.setItem("smartRoutingEagerness", value);
  });

  const setLean = $((value: "speed" | "balanced" | "quality") => {
    routingLean.value = value;
    localStorage.setItem("routingOfflineLean", value);
  });

  // Factory reset — wipe local AIs/conversations/memory and restore defaults.
  const resetModalOpen = useSignal(false);
  const resetting = useSignal(false);
  const doReset = $(async () => {
    resetting.value = true;
    try {
      // Clear all UI preferences (theme, model choice, help tips, memory queue…).
      // The Flowsta account lives in a Rust-side store, not localStorage, so this
      // doesn't sign the user out.
      localStorage.clear();
      const { invoke } = await import("@tauri-apps/api/core");
      // Wipes local data and relaunches — this call doesn't return normally.
      await invoke("reset_to_defaults");
    } catch (e) {
      console.error("[Settings] reset failed:", e);
      resetting.value = false;
      resetModalOpen.value = false;
    }
  });

  const jumpTo = $((id: string) => {
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
  });

  const handleNewQuestion = $(() => {
    nav("/chat");
  });

  const handleModelsClick = $(() => {
    nav("/setup");
  });

  return (
    <div class="min-h-screen flex flex-col bg-[var(--bg-main)]">
      <AppHeader
        handleNewQuestion$={handleNewQuestion}
        handleModelsClick$={handleModelsClick}
        currentModel={currentModel.value}
        showModelWidget={showModelWidget.value && currentModel.value !== null}
      />

      <div class="flex-1 overflow-y-auto">
        <div class="max-w-5xl mx-auto px-4 py-8">
          {/* Page title */}
          <h1 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-6 border-b border-[var(--border-subtle)] pb-2">
            Settings
          </h1>

          <div class="flex gap-6 items-start">
            {/* Section index — jump nav, sticky beside the sections. */}
            <nav class="hidden md:flex flex-col gap-0.5 w-44 flex-shrink-0 sticky top-4">
              {SECTIONS.map((s) => (
                <button
                  key={s.id}
                  onClick$={() => jumpTo(s.id)}
                  class="text-left text-sm px-3 py-1.5 rounded-lg text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-card)] transition-colors"
                >
                  {s.label}
                </button>
              ))}
            </nav>

            {/* Settings sections — identity & protection, then how your AI
                behaves, then capability infrastructure, then app chrome. */}
            <div class="flex-1 min-w-0 space-y-6">
              {/* Your Flowsta Account — first: what connecting gets you. */}
              <div id="settings-account" class="scroll-mt-4">
                <FlowstaAccount section="account" />
              </div>

              {/* Backups & recovery — the payoff, right below the relationship. */}
              <div id="settings-backups" class="scroll-mt-4">
                <FlowstaAccount section="backups" />
              </div>

              {/* AI behavior */}
              <section
                id="settings-behavior"
                class="scroll-mt-4 bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]"
              >
                <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-4">
                  AI behavior
                </h2>
                <div class="space-y-4">
                  <SettingToggle
                    title="Smart mode detection"
                    checked={smartModeDetection}
                    onToggle$={toggleSmartMode}
                  >
                    When on, your AI notices when you ask for a report or for code
                    and switches automatically — no need to pick it from the sparkle
                    menu. It runs a quick check on your own model only when a message
                    looks like a request, so normal chat stays instant. Turn off for
                    fully manual control.
                  </SettingToggle>
                  <SettingToggle
                    title="Verify answers against documents"
                    checked={groundDocumentsAuto}
                    onToggle$={toggleGroundDocuments}
                  >
                    When on, after every answer about an attached document your AI
                    checks each claim against the source and links the exact
                    supporting quote. It's thorough but adds a local pass that can
                    take a while. When off, you'll get a "Verify sources" button on
                    document answers to run it on demand.
                  </SettingToggle>
                  <SettingToggle
                    title="Send attachments to online models"
                    checked={allowAttachmentsOnline}
                    onToggle$={toggleAttachmentsOnline}
                  >
                    When on, images and files go to online (cloud) models without
                    asking each time. Off means you're asked first, before anything
                    leaves your device. Offline models always stay local.
                  </SettingToggle>
                  {buildInstalled.value && (
                    <SettingToggle
                      title="Show thinking while working in a folder"
                      checked={agentShowThoughts}
                      onToggle$={toggleAgentShowThoughts}
                    >
                      When your AI is working in a folder, show its live reasoning
                      between steps - the running commentary of what it's considering
                      and why. The status line also carries the tail of its current
                      thought. Same switch as the brain icon on the working steps box.
                    </SettingToggle>
                  )}
                </div>
              </section>

              {/* Memory — what your AIs remember and where saves go. */}
              <section
                id="settings-memory"
                class="scroll-mt-4 bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]"
              >
                <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-2">
                  Memory
                </h2>
                <p class="text-sm text-[var(--text-secondary)] mb-4">
                  Your AIs remember in two places: each AI's own memory (what it
                  learned and what you gave it), and the shared notes on{" "}
                  <a href="/your-memory/" class="text-[var(--text-link)] hover:underline">
                    Your Memory
                  </a>{" "}
                  that every AI can draw on. These settings choose where each
                  "Remember this" action saves to.
                </p>

                <div class="space-y-5">
                  <RememberDestinationPicker
                    title="Remembering highlighted text"
                    hint='The "Remember" chip that appears when you highlight part of a reply.'
                    surface="selection"
                    scope={rememberScopeSelection}
                  />
                  <RememberDestinationPicker
                    title="Remembering whole replies"
                    hint='The "Remember" button under a reply, and on transcript entries on the memory page.'
                    surface="reply"
                    scope={rememberScopeReply}
                  />

                  <div class="border-t border-[var(--border-subtle)] pt-4">
                    <SettingToggle
                      title="Automatic learning"
                      checked={memoryLearning}
                      onToggle$={toggleMemoryLearning}
                    >
                      Your AIs pick up facts you share in conversation (your
                      name, preferences, projects) so you don't repeat yourself.
                      Turn off to remember only what you save yourself - what's
                      already learned is kept and managed on Your Memory.
                    </SettingToggle>
                  </div>

                  <p class="text-xs text-[var(--text-muted)]">
                    Remembering and recall use the small on-device memory model
                    - download or remove it under Components below. Everything
                    here stays on your device.
                  </p>
                </div>
              </section>

              {/* Routing — eagerness today; the planned routing knobs land here. */}
              <section
                id="settings-routing"
                class="scroll-mt-4 bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]"
              >
                <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-2">
                  Routing
                </h2>
                <p class="text-sm text-[var(--text-secondary)] mb-4">
                  How the Auto modes pick a model for each question. AIs set to a
                  specific model are never affected.
                </p>

                {/* On your device — biases the fit-aware offline pick. */}
                <h3 class="text-lg font-semibold text-[var(--text-primary)]">
                  On your device
                </h3>
                <p class="text-sm text-[var(--text-secondary)] mt-1 mb-3">
                  Applies to every Auto mode: which of your downloaded models the
                  automatic pick leans toward.
                </p>
                <div class="grid grid-cols-1 sm:grid-cols-3 gap-2 mb-2">
                  {[
                    { id: "speed", label: "Prefer fastest", hint: "A model that fits fully on your GPU wins" },
                    { id: "balanced", label: "Balanced", hint: "Best capability that still runs well" },
                    { id: "quality", label: "Prefer strongest", hint: "The most capable model, even if slower" },
                  ].map((opt) => (
                    <button
                      key={opt.id}
                      onClick$={() => setLean(opt.id as "speed" | "balanced" | "quality")}
                      class={`text-left rounded-xl p-3 border transition-colors ${
                        routingLean.value === opt.id
                          ? "bg-[var(--bg-button-primary)] text-[var(--text-button-primary)] border-[var(--border-subtle)]"
                          : "bg-[var(--bg-main)] text-[var(--text-primary)] border-[var(--border-subtle)] hover:opacity-90"
                      }`}
                      aria-pressed={routingLean.value === opt.id}
                    >
                      <div class="font-semibold text-sm">{opt.label}</div>
                      <div
                        class={`text-xs mt-1 ${
                          routingLean.value === opt.id ? "" : "text-[var(--text-secondary)]"
                        }`}
                      >
                        {opt.hint}
                      </div>
                    </button>
                  ))}
                </div>
                <p class="text-xs text-[var(--text-muted)] mb-5">
                  Tip: pausing a model on the Offline Models page also removes it
                  from the Auto pool.
                </p>

                <div class="border-t border-[var(--border-subtle)] my-5" />

                {/* Online routing controls are only OFFERED with a plan — a
                    quiet unlock note otherwise (settings already made keep
                    working; billing is enforced per request). */}
                {!onlineEntitled.value ? (
                  <p class="text-sm text-[var(--text-muted)]">
                    Online routing — sending fresh-info or hard questions to
                    online models — unlocks with a plan. Set it up on the
                    Online Models page.
                  </p>
                ) : (
                <>
                <h3 class="text-lg font-semibold text-[var(--text-primary)]">
                  Online
                </h3>
                <p class="text-sm text-[var(--text-secondary)] mt-1 mb-4">
                  For AIs set to "Auto — Online and Offline". A question goes
                  online only when your device can't do it justice: it needs
                  current information from the web, or it's genuinely hard.
                  Everything else stays on your device. "Offline Only" AIs
                  never go online.
                </p>

                {/* Online eagerness — unchanged shipped knob. */}
                <h4 class="text-base font-semibold text-[var(--text-primary)]">
                  Going online for fresh information
                </h4>
                <p class="text-sm text-[var(--text-secondary)] mt-1 mb-3">
                  How eagerly a question that may need up-to-date information
                  goes to an online model.
                </p>
                <div class="grid grid-cols-1 sm:grid-cols-3 gap-2 mb-5">
                  {[
                    { id: "privacy", label: "Privacy-first", hint: "Only clearly current questions go online" },
                    { id: "balanced", label: "Balanced", hint: "Online when a question likely needs fresh info" },
                    { id: "freshness", label: "Freshness-first", hint: "Lean online whenever it might help" },
                  ].map((opt) => (
                    <button
                      key={opt.id}
                      onClick$={() => setEagerness(opt.id as "privacy" | "balanced" | "freshness")}
                      class={`text-left rounded-xl p-3 border transition-colors ${
                        routingEagerness.value === opt.id
                          ? "bg-[var(--bg-button-primary)] text-[var(--text-button-primary)] border-[var(--border-subtle)]"
                          : "bg-[var(--bg-main)] text-[var(--text-primary)] border-[var(--border-subtle)] hover:opacity-90"
                      }`}
                      aria-pressed={routingEagerness.value === opt.id}
                    >
                      <div class="font-semibold text-sm">{opt.label}</div>
                      <div
                        class={`text-xs mt-1 ${
                          routingEagerness.value === opt.id ? "" : "text-[var(--text-secondary)]"
                        }`}
                      >
                        {opt.hint}
                      </div>
                    </button>
                  ))}
                </div>

                {/* Per-slot online model picks. Escalation itself has no
                    toggle: choosing online-offline mode IS the consent to go
                    online when it helps (auto:Offline is the never-online
                    choice). Recommended names must match the router's
                    DEFAULT_* ids in router.rs. */}
                <h4 class="text-base font-semibold text-[var(--text-primary)] mt-5">
                  Which online model answers
                </h4>
                <p class="text-sm text-[var(--text-secondary)] mt-1 mb-1">
                  When a question does go online, these are the models that take
                  it. Recommended picks are chosen for each job; change them if
                  you'd rather use a different model.
                </p>
                <OnlineModelPicker
                  label="Needs current information"
                  hint="Searches the live web and cites sources"
                  recommended="Grok 4.5 (Web)"
                  storageKey="routingOnlineFresh"
                  selected={onlineFresh}
                  models={onlineModels}
                />
                <OnlineModelPicker
                  label="Hard coding, reasoning, or math"
                  hint="The toughest technical questions"
                  recommended="GPT-5.6 Sol"
                  storageKey="routingOnlineHardCode"
                  selected={onlineHardCode}
                  models={onlineModels}
                />
                <OnlineModelPicker
                  label="Other hard questions"
                  hint="Genuinely difficult questions outside those areas"
                  recommended="GPT-5.6 Terra"
                  storageKey="routingOnlineHardGeneral"
                  selected={onlineHardGeneral}
                  models={onlineModels}
                />
                </>
                )}
              </section>

              {/* On-demand capability models (memory recall, etc.) */}
              <div id="settings-components" class="scroll-mt-4">
                <ComponentsSettings />
              </div>

              {/* Inference engines (bundled + optional hardware-specific) */}
              <div id="settings-engines" class="scroll-mt-4">
                <EnginesSettings />
              </div>

              {/* External app access (local OpenAI-compatible inference endpoint) */}
              <div id="settings-external" class="scroll-mt-4">
                <ExternalAccess />
              </div>

              {/* Appearance */}
              <section
                id="settings-appearance"
                class="scroll-mt-4 bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]"
              >
                <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-4">
                  Appearance
                </h2>
                <div class="space-y-4">
                  <SettingToggle
                    title="Offline Model Loading Widget in Header"
                    checked={showModelWidget}
                    onToggle$={toggleModelWidget}
                  >
                    Show the current model name and loading status in the header
                  </SettingToggle>
                  <SettingToggle
                    title="Show help tips"
                    checked={showHelpTips}
                    onToggle$={toggleHelpTips}
                  >
                    The short "what this does" info boxes around the app.{" "}
                    <button
                      type="button"
                      onClick$={resetTips}
                      class="text-[var(--text-link)] hover:underline"
                    >
                      Show dismissed tips again
                    </button>
                  </SettingToggle>
                </div>
              </section>

              {/* Reset to defaults */}
              <section
                id="settings-reset"
                class="scroll-mt-4 bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]"
              >
                <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-2">
                  Reset
                </h2>
                <p class="text-sm text-[var(--text-secondary)] mb-4">
                  Restore the app to a fresh start: removes all your AIs,
                  conversations, and learned memory, and brings back the default
                  AIs. Your Flowsta account and downloaded models are kept. This
                  can't be undone.
                </p>
                <LiquidMetalButton
                  variant="danger"
                  onClick$={() => (resetModalOpen.value = true)}
                  class="px-4 py-2 text-sm"
                >
                  Reset to defaults
                </LiquidMetalButton>
              </section>
            </div>
          </div>
        </div>
      </div>

      <ConfirmModal
        isOpen={resetModalOpen.value}
        title="Reset to defaults?"
        message="This permanently deletes all your AIs, conversations, and learned memory, and restores the default AIs. Your Flowsta account and downloaded models are kept. This can't be undone, and the app will restart."
        confirmLabel="Reset everything"
        cancelLabel="Cancel"
        variant="danger"
        busy={resetting.value}
        onConfirm$={doReset}
        onCancel$={() => (resetModalOpen.value = false)}
      />

      {/* Version number in bottom right corner */}
      <div class="fixed bottom-4 right-4 text-xs text-[var(--text-secondary)] opacity-50 hover:opacity-100 transition-opacity">
        v{appVersion.value}
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Settings - Your Own AI",
};
