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
import { useHeaderWorkspace } from "../../hooks/useHeaderWorkspace";
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
import {
  ensureMedicalModel,
  isMedicalSpecialist,
  medicalOnlineAlways,
  setMedicalModel,
  setMedicalOnlineAlways,
} from "../../utils/medicalModel";
import { modelManager } from "../../utils/modelManager";
import {
  AUTO_PERMISSIONS_COPY,
  defaultPermissionMode,
  judgeEnabled,
  setDefaultPermissionMode,
  setJudgeEnabled,
} from "../../utils/agentPermissions";
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
              <div class="px-3 pt-1.5 pb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)]">
                Online models
              </div>
              <button
                type="button"
                onClick$={async () => {
                  props.selected.value = "";
                  localStorage.removeItem(props.storageKey);
                  open.value = false;
                  try {
                    const { Store } = await import("@tauri-apps/plugin-store");
                    const store = await Store.load("settings.json");
                    await store.set(props.storageKey, "");
                    await store.save();
                  } catch { /* store mirror is best-effort */ }
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
                  onClick$={async () => {
                    props.selected.value = m.id;
                    localStorage.setItem(props.storageKey, m.id);
                    open.value = false;
                    try {
                      const { Store } = await import("@tauri-apps/plugin-store");
                      const store = await Store.load("settings.json");
                      await store.set(props.storageKey, m.id);
                      await store.save();
                    } catch { /* store mirror is best-effort */ }
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
  { id: "settings-agent", label: "Agent" },
  { id: "settings-components", label: "Components" },
  { id: "settings-engines", label: "Engines" },
  { id: "settings-external", label: "External access" },
  { id: "settings-appearance", label: "Appearance" },
  { id: "settings-diagnostics", label: "Help & diagnostics" },
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
  const headerWs = useHeaderWorkspace();

  const showModelWidget = useSignal(false);
  const showChatModelChip = useSignal(true);
  const showHelpTips = useSignal(true);
  const allowAttachmentsOnline = useSignal(false);
  // Agent: auto permissions default for new projects + the model judge - both off unless set.
  const agentAutoDefault = useSignal(false);
  const agentJudge = useSignal(false);
  // Health questions: which installed model answers them (always offline).
  const medicalModel = useSignal<string>("");
  const installedChatModels = useSignal<string[]>([]);
  const onlineModelIds = useSignal<{ id: string; name: string }[]>([]);
  const medicalOnlineAlwaysSig = useSignal(false);
  const groundDocumentsAuto = useSignal(false);
  const smartModeDetection = useSignal(true);
  // Same key the working box's brain icon toggles - one setting, two doors.
  // Only shown when the Build agent is actually installed.
  const agentSimpleView = useSignal(false);
  const buildInstalled = useSignal(false);
  // "Run in your terminal" behavior: default = pre-filled, Enter to run.
  const terminalRunImmediately = useSignal(false);
  const currentModel = useSignal<string | null>(null);
  const rememberScopeSelection = useSignal<RememberScope>("per-ai");
  const rememberScopeReply = useSignal<RememberScope>("per-ai");
  // "Automatic learning" = NOT paused (the toggle reads positively; the
  // stored flag is the pause, shared with the Your Memory page).
  const memoryLearning = useSignal(true);
  const routingEagerness = useSignal<"privacy" | "balanced" | "freshness">("balanced");
  const routingLean = useSignal<"speed" | "balanced" | "quality">("balanced");
  // Per-slot online model overrides ("" = the router's recommended default).
  const onlineAgent = useSignal("");
  const onlinePlanning = useSignal("");
  // Project cost levers (router reads the store mirror at session start).
  const projectDeviceSubagents = useSignal(true);
  const projectThrifty = useSignal(false);
  const routingExplainerOpen = useSignal(false);
  const routingDecisions = useSignal<{ at_ms: number; model: string; reason: string }[]>([]);
  const onlineFresh = useSignal("");
  const onlineHardCode = useSignal("");
  const onlineHardGeneral = useSignal("");
  const onlineModels = useSignal<{ id: string; display_name: string }[]>([]);
  // Tool-capable subset for the Agent slot (a tools-blind pick would mean
  // broken folder sessions - the picker only offers models that can drive).
  const onlineAgentModels = useSignal<{ id: string; display_name: string }[]>([]);
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
    // Default ON - only an explicit "false" hides the chip.
    showChatModelChip.value =
      localStorage.getItem("showChatModelChip") !== "false";
    showHelpTips.value = helpTipsEnabled();
    allowAttachmentsOnline.value =
      localStorage.getItem("allowAttachmentsOnline") === "true";
    agentAutoDefault.value = defaultPermissionMode() === "auto";
    agentJudge.value = judgeEnabled();
    (async () => {
      try {
        installedChatModels.value = (await modelManager.listModels()).map((m) => m.name);
        medicalModel.value = (await ensureMedicalModel()) ?? "";
        medicalOnlineAlwaysSig.value = medicalOnlineAlways();
      } catch {
        /* models list not reachable = leave the picker empty */
      }
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const online = await invoke<{ id: string; display_name?: string }[]>("list_online_models");
        onlineModelIds.value = online.map((m) => ({ id: m.id, name: m.display_name ?? m.id.replace(/^online:/, "") }));
      } catch {
        /* offline / signed out = no online options */
      }
    })();
    groundDocumentsAuto.value =
      localStorage.getItem("groundDocumentsAuto") === "true";
    smartModeDetection.value =
      localStorage.getItem("smartModeDetection") !== "false"; // default ON
    // "Simple project view" INVERTS the stored key: absent/"1" = full
    // detail (the default - the living rail is the product); "0" = simple.
    agentSimpleView.value = localStorage.getItem("agent-show-thoughts") === "0";
    terminalRunImmediately.value =
      localStorage.getItem("terminal-run-immediately") === "true";
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
    onlineAgent.value = localStorage.getItem("routingOnlineAgent") || "";
    projectDeviceSubagents.value =
      localStorage.getItem("routingProjectDeviceSubagents") !== "0";
    projectThrifty.value = localStorage.getItem("routingProjectThrifty") === "1";
    onlinePlanning.value = localStorage.getItem("routingOnlinePlanning") || "";
    import("@tauri-apps/api/core")
      .then(({ invoke }) =>
        invoke<{ at_ms: number; model: string; reason: string }[]>(
          "recent_routing_decisions",
        ),
      )
      .then((d) => (routingDecisions.value = d))
      .catch(() => {});
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
          Promise.all(
            onlineModels.value.map(async (m) => {
              const { invoke } = await import("@tauri-apps/api/core");
              return {
                m,
                cap: await invoke<number>("agent_capability", { model: m.id }).catch(() => 0),
              };
            }),
          ).then((scored) => {
            onlineAgentModels.value = scored.filter((x) => x.cap >= 6).map((x) => x.m);
          });
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

  const toggleChatModelChip = $(() => {
    showChatModelChip.value = !showChatModelChip.value;
    localStorage.setItem(
      "showChatModelChip",
      showChatModelChip.value.toString()
    );
    window.dispatchEvent(
      new CustomEvent("settingsChanged", {
        detail: { showChatModelChip: showChatModelChip.value },
      })
    );
  });

  const toggleHelpTips = $(() => {
    showHelpTips.value = !showHelpTips.value;
    setHelpTipsEnabled(showHelpTips.value);
  });

  const toggleAgentAutoDefault = $(() => {
    agentAutoDefault.value = !agentAutoDefault.value;
    setDefaultPermissionMode(agentAutoDefault.value ? "auto" : "ask");
  });
  const toggleAgentJudge = $(() => {
    agentJudge.value = !agentJudge.value;
    setJudgeEnabled(agentJudge.value);
  });

  const toggleAttachmentsOnline = $(async () => {
    allowAttachmentsOnline.value = !allowAttachmentsOnline.value;
    localStorage.setItem(
      "allowAttachmentsOnline",
      allowAttachmentsOnline.value.toString()
    );
    // Mirror into the store the Rust router reads, so agent turns (routed
    // in the inference server) honor the same answer as direct chats.
    try {
      const { Store } = await import("@tauri-apps/plugin-store");
      const store = await Store.load("settings.json");
      await store.set("allowAttachmentsOnline", allowAttachmentsOnline.value);
      await store.save();
    } catch { /* store mirror is best-effort */ }
  });

  const toggleMemoryLearning = $(() => {
    memoryLearning.value = !memoryLearning.value;
    setMemoryPaused(!memoryLearning.value);
  });

  const toggleSmartMode = $(() => {
    smartModeDetection.value = !smartModeDetection.value;
    localStorage.setItem("smartModeDetection", smartModeDetection.value.toString());
  });

  const toggleTerminalImmediate = $(() => {
    terminalRunImmediately.value = !terminalRunImmediately.value;
    localStorage.setItem(
      "terminal-run-immediately",
      terminalRunImmediately.value.toString(),
    );
  });

  const toggleAgentSimpleView = $(() => {
    agentSimpleView.value = !agentSimpleView.value;
    localStorage.setItem("agent-show-thoughts", agentSimpleView.value ? "0" : "1");
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

  // Help & diagnostics
  /** What this OS calls its crash evidence, for the section description. */
  const crashRecordsName = useSignal("your system's crash records for this app");
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    const ua = navigator.userAgent;
    crashRecordsName.value = ua.includes("Windows")
      ? "Windows' crash records for this app"
      : ua.includes("Mac")
        ? "macOS's crash reports for this app"
        : "your system's crash journal entries for this app";
  });
  const diagBusy = useSignal(false);
  const diagSavedPath = useSignal("");
  const diagError = useSignal("");
  const diagCopied = useSignal(false);

  // Clipboard variant: no file to find and attach - paste straight into an
  // email or chat. Copying happens on the Rust side (same report builder).
  const copyDiagnostics = $(async () => {
    diagError.value = "";
    diagCopied.value = false;
    diagBusy.value = true;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke<number>("copy_diagnostics");
      diagCopied.value = true;
    } catch (e) {
      diagError.value = String(e);
    } finally {
      diagBusy.value = false;
    }
  });

  const saveDiagnostics = $(async () => {
    diagError.value = "";
    try {
      const [{ save }, { invoke }] = await Promise.all([
        import("@tauri-apps/plugin-dialog"),
        import("@tauri-apps/api/core"),
      ]);
      const date = new Date().toISOString().slice(0, 10);
      const path = await save({
        defaultPath: `your-own-ai-diagnostics-${date}.txt`,
        filters: [{ name: "Text file", extensions: ["txt"] }],
      });
      if (!path) return;
      diagBusy.value = true;
      diagSavedPath.value = await invoke<string>("export_diagnostics", { path });
    } catch (e) {
      diagError.value = String(e);
    } finally {
      diagBusy.value = false;
    }
  });

  const revealDiagnostics = $(async () => {
    try {
      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      await revealItemInDir(diagSavedPath.value);
    } catch {
      // Reveal is a convenience; the saved-path text remains the fallback.
    }
  });
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
                    title="Run commands immediately"
                    checked={terminalRunImmediately}
                    onToggle$={toggleTerminalImmediate}
                  >
                    "Run in your terminal" on a suggested command normally
                    types it onto your prompt, ready to edit and run with
                    Enter - the final look stays with you. Turn this on to
                    execute the moment you click instead.
                  </SettingToggle>
                  {buildInstalled.value && (
                    <SettingToggle
                      title="Simple project view"
                      checked={agentSimpleView}
                      onToggle$={toggleAgentSimpleView}
                    >
                      Show just the steps, asks, and plan while your AI works -
                      without the running thoughts and live task logs (expanding
                      a step still shows its log). Everything keeps moving either
                      way. Same switch as the brain icon on the working steps.
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

                {/* Transparency, not controls: the knobless smarts described
                    plainly. Permanent reference (no "Got it" dismissal - this
                    is not a one-time tip), collapsible to stay quiet. */}
                <div class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-main)] mb-5">
                  <button
                    onClick$={() => (routingExplainerOpen.value = !routingExplainerOpen.value)}
                    class="flex w-full items-center justify-between px-4 py-3 text-left"
                  >
                    <span class="text-sm font-semibold text-[var(--text-primary)]">
                      What routing does automatically
                    </span>
                    <LuChevronDown
                      class={`w-4 h-4 text-[var(--text-muted)] transition-transform ${routingExplainerOpen.value ? "rotate-180" : ""}`}
                    />
                  </button>
                  {routingExplainerOpen.value && (
                    <div class="px-4 pb-4 text-sm text-[var(--text-secondary)] space-y-2">
                      <p>
                        Health questions go to the model you chose under
                        Health questions below - on your device by default,
                        and never online without asking you first.
                      </p>
                      <p>Questions that need current information go to a live-web model, at the eagerness you set below.</p>
                      <p>Genuinely hard questions can be passed to a stronger online model (Auto - Online and Offline only).</p>
                      <p>Plain conversation asks a model for quick, light thinking; reports, code, and genuinely hard questions get the deep kind - online and on your device alike.</p>
                      <p>Project work only ever uses models that can drive tools, and never switches models mid-session.</p>
                      <p>Project sessions default to the recommended tool-driver online - both project pickers below let you change that.</p>
                      <p>Simple project side-work (searching and reading fan-outs) runs on your device when a capable model runs comfortably on your hardware - free and private. You can turn this off below.</p>
                      <p>The small jobs inside project work - summaries, tidying the conversation memory - always run on your device.</p>
                      <p>Picks are fit-aware: a model that runs well on your hardware beats a stronger one that struggles. A loaded model that struggles hands off to one that runs at full speed - except while a project is open, so the session's model stays warm.</p>
                      <p>Models you pause on the Offline or Online Models pages are never auto-picked - pausing is your veto.</p>
                      <p>A question with an image or document attached stays on your device unless you've turned on "Send attachments to online models" above - then online models may take it when they'd do better.</p>
                      <p>Every model starts with as much room to read and remember as your graphics card and memory can carry.</p>
                      <p class="text-[var(--text-muted)]">
                        Every answer's Model button shows what happened and why, and each
                        decision is stored in the conversation's tamper-proof record.
                      </p>
                      {routingDecisions.value.length > 0 && (
                        <div class="pt-2 border-t border-[var(--border-subtle)]">
                          <div class="text-xs font-semibold text-[var(--text-primary)] mb-1.5">
                            Recent decisions
                          </div>
                          <div class="space-y-1">
                            {routingDecisions.value.slice(0, 8).map((d, i) => (
                              <div key={i} class="text-xs text-[var(--text-muted)] truncate">
                                <span class="text-[var(--text-secondary)]">
                                  {d.model.replace("online:", "").replace(/\.gguf$/i, "")}
                                </span>
                                {" - "}
                                {d.reason}
                              </div>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  )}
                </div>

                {/* On your device — biases the fit-aware offline pick. */}
                <h3 class="text-lg font-semibold text-[var(--text-primary)]">
                  Choosing a model on this device
                </h3>
                <p class="text-sm text-[var(--text-secondary)] mt-1 mb-3">
                  Both Auto modes pick from the models you've downloaded (the
                  Offline Models page is their library). This sets what the
                  pick leans toward.
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
                  Going online (Auto — Online and Offline)
                </h3>
                <p class="text-sm text-[var(--text-secondary)] mt-1 mb-4">
                  Only for AIs set to "Auto — Online and Offline". A question goes
                  online only when your device can't do it justice: it needs
                  current information from the web, or it's genuinely hard.
                  Everything else stays on your device. "Offline Only" AIs
                  never go online.
                </p>

                {/* Attachments: one control with one meaning - the standing
                    answer to "may my documents and images go to online
                    models?". Off (default): the router keeps attachment turns
                    on your device, and picking an online model by hand asks
                    first. On: the router may route them online when that is
                    better, with no per-turn ask. */}
                <div class="mb-5">
                  <SettingToggle
                    title="Send attachments to online models"
                    checked={allowAttachmentsOnline}
                    onToggle$={toggleAttachmentsOnline}
                  >
                    Off: a question with an image or document attached stays on
                    your device, even for an "Auto — Online and Offline" AI, and
                    choosing an online model yourself asks before anything
                    leaves. On: online models may be used for attachments when
                    they'd do a better job, without asking each time. Offline
                    AIs never send anything anywhere.
                  </SettingToggle>
                </div>

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

                {/* Health questions: always answered on the device; this
                    picks WHICH installed model answers them. Starts as the
                    first model downloaded; installing a medical specialist
                    asks once whether to switch (never a silent hijack). */}
                {installedChatModels.value.length > 0 && (
                  <>
                    <h4 class="text-base font-semibold text-[var(--text-primary)] mt-5">
                      Health questions
                    </h4>
                    <p class="text-sm text-[var(--text-secondary)] mt-1 mb-2">
                      Health questions stay on your device by default. This
                      chooses which model answers them - a medical model gives
                      more grounded health answers when you have one. Choosing
                      an online model is possible too: each health question
                      then asks before anything leaves your device.
                    </p>
                    <select
                      class="w-full sm:w-auto rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-main)] px-3 py-2 text-sm text-[var(--text-primary)] mb-2"
                      value={medicalModel.value}
                      onChange$={async (_, el) => {
                        medicalModel.value = el.value;
                        await setMedicalModel(el.value);
                      }}
                    >
                      <optgroup label="On your device">
                        {installedChatModels.value.map((name) => (
                          <option key={name} value={name} selected={name === medicalModel.value}>
                            {name.replace(/\.gguf$/i, "") + (isMedicalSpecialist(name) ? "  (medical)" : "")}
                          </option>
                        ))}
                      </optgroup>
                      {onlineModelIds.value.length > 0 && (
                        <optgroup label="Online - asks before each question">
                          {onlineModelIds.value.map((m) => (
                            <option key={m.id} value={m.id} selected={m.id === medicalModel.value}>
                              {m.name}
                            </option>
                          ))}
                        </optgroup>
                      )}
                    </select>
                    {medicalModel.value.startsWith("online:") && (
                      <p class="text-xs text-[var(--text-muted)] mb-5">
                        {medicalOnlineAlwaysSig.value ? (
                          <>
                            Sending health questions online without asking
                            (you chose "always").{" "}
                            <button
                              class="underline"
                              onClick$={() => {
                                setMedicalOnlineAlways(false);
                                medicalOnlineAlwaysSig.value = false;
                              }}
                            >
                              Ask again each time
                            </button>
                          </>
                        ) : (
                          <>
                            Each health question shows a card before it goes
                            online - answer on your device stays one tap away.
                          </>
                        )}
                      </p>
                    )}
                    {!medicalModel.value.startsWith("online:") && <span class="block mb-3" />}
                  </>
                )}

                {/* Per-slot online model picks. Escalation itself has no
                    toggle: choosing online-offline mode IS the consent to go
                    online when it helps (auto:Offline is the never-online
                    choice). Recommended names must match the router's
                    DEFAULT_* ids in router.rs. */}
                <h4 class="text-base font-semibold text-[var(--text-primary)] mt-5">
                  Which online model answers
                </h4>
                <p class="text-sm text-[var(--text-secondary)] mt-1 mb-1">
                  When a question does go online, these are the online models
                  that take it. Recommended picks are chosen for each job;
                  change them if you'd rather use a different one. Your
                  downloaded models aren't listed here - they're picked
                  automatically, tuned under "Choosing a model on this
                  device".
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
                {buildInstalled.value && (
                  <>
                    <div class="border-t border-[var(--border-subtle)] my-5" />
                    <h4 class="text-base font-semibold text-[var(--text-primary)]">
                      Working on projects
                    </h4>
                    <p class="text-sm text-[var(--text-secondary)] mt-1 mb-3">
                      Project work routes differently: only models that can drive
                      tools are ever used, and the model never changes
                      mid-session. Offline, the most capable tool-driving model
                      you've downloaded takes it. Online, this one does:
                    </p>
                    <OnlineModelPicker
                      label="Agent work on projects"
                      hint="The online model that drives tools in project work - only capable ones are listed"
                      recommended="GPT-5.6 Sol"
                      storageKey="routingOnlineAgent"
                      selected={onlineAgent}
                      models={onlineAgentModels}
                    />
                    <OnlineModelPicker
                      label="Planning and helper agents"
                      hint="The online model for the subagents that explore and plan - reasoning-lean, still tool-capable"
                      recommended="GPT-5.6 Sol"
                      selected={onlinePlanning}
                      storageKey="routingOnlinePlanning"
                      models={onlineAgentModels}
                    />
                    {/* Cost levers - both mirrored to the Rust-readable
                        store; the router reads them at session start. */}
                    <label class="flex items-start gap-3 mt-4 cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={projectDeviceSubagents.value}
                        onChange$={async (_, el) => {
                          projectDeviceSubagents.value = el.checked;
                          localStorage.setItem("routingProjectDeviceSubagents", el.checked ? "1" : "0");
                          try {
                            const { Store } = await import("@tauri-apps/plugin-store");
                            const store = await Store.load("settings.json");
                            await store.set("routingProjectDeviceSubagents", el.checked ? "1" : "0");
                            await store.save();
                          } catch { /* store mirror is best-effort */ }
                        }}
                        class="rounded mt-0.5"
                      />
                      <span>
                        <span class="block text-sm text-[var(--text-primary)]">
                          Do simple project side-work on this device
                        </span>
                        <span class="block text-xs text-[var(--text-muted)]">
                          Searching and reading fan-outs run on your device when a
                          capable model runs comfortably on your hardware - free
                          and private. Otherwise they use the session's online
                          model. Takes effect next project open.
                        </span>
                      </span>
                    </label>
                    <label class="flex items-start gap-3 mt-3 cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={projectThrifty.value}
                        onChange$={async (_, el) => {
                          projectThrifty.value = el.checked;
                          localStorage.setItem("routingProjectThrifty", el.checked ? "1" : "0");
                          try {
                            const { Store } = await import("@tauri-apps/plugin-store");
                            const store = await Store.load("settings.json");
                            await store.set("routingProjectThrifty", el.checked ? "1" : "0");
                            await store.save();
                          } catch { /* store mirror is best-effort */ }
                        }}
                        class="rounded mt-0.5"
                      />
                      <span>
                        <span class="block text-sm text-[var(--text-primary)]">
                          Keep whole project sessions on this device when possible
                        </span>
                        <span class="block text-xs text-[var(--text-muted)]">
                          When a capable model fits your hardware, project
                          sessions stay fully on-device - slower, but free and
                          private. Otherwise they go online as usual.
                        </span>
                      </span>
                    </label>
                  </>
                )}
                </>
                )}
              </section>

              {/* Agent - how the coding/project agent asks before it acts.
                  Both switches are OFF unless the user turns them on: the
                  cautious default. Per-project choice lives on the project
                  chip in the header and wins over the default here. */}
              <section
                id="settings-agent"
                class="scroll-mt-4 bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]"
              >
                <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-2">
                  Agent
                </h2>
                <p class="text-sm text-[var(--text-secondary)] mb-4">
                  When your AI works in a project folder it asks before it runs
                  commands or changes files. These set the default for projects
                  you open from now on; each project can choose its own from the
                  project chip in the header. Everything the agent is allowed to
                  do - by you, or by these settings - is written to your records.
                </p>
                <div class="space-y-5">
                  <SettingToggle
                    title={AUTO_PERMISSIONS_COPY.title}
                    checked={agentAutoDefault}
                    onToggle$={toggleAgentAutoDefault}
                  >
                    {AUTO_PERMISSIONS_COPY.body} Off (default): your AI asks
                    every time.
                  </SettingToggle>
                  <SettingToggle
                    title={AUTO_PERMISSIONS_COPY.judgeTitle}
                    checked={agentJudge}
                    onToggle$={toggleAgentJudge}
                  >
                    {AUTO_PERMISSIONS_COPY.judgeBody} Only matters when auto
                    permissions are on.
                  </SettingToggle>
                </div>
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
                    title="Model chip in chat"
                    checked={showChatModelChip}
                    onToggle$={toggleChatModelChip}
                  >
                    The small model name beside the Ask row - shows which model
                    arrangement the current AI uses, tap to change it. Turn off
                    for a cleaner chat; you can always switch models from the
                    Your AIs page.
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

              {/* Help & diagnostics */}
              <section
                id="settings-diagnostics"
                class="scroll-mt-4 bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]"
              >
                <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-2">
                  Help &amp; diagnostics
                </h2>
                <p class="text-sm text-[var(--text-secondary)] mb-4">
                  <span class="font-semibold text-[var(--text-primary)]">
                    Save diagnostic report
                  </span>{" "}
                  - bundles the app's own logs, your system details, and{" "}
                  {crashRecordsName.value} into one readable text file. It never
                  includes your conversations, memories, or keys - open it in
                  any text editor and see exactly what you're sharing. Nothing
                  is sent anywhere; you choose who gets the file.
                </p>
                <div class="flex flex-wrap items-center gap-3">
                  <LiquidMetalButton
                    variant="secondary"
                    onClick$={copyDiagnostics}
                    class="px-4 py-2 text-sm"
                  >
                    {diagCopied.value ? "Copied" : "Copy to clipboard"}
                  </LiquidMetalButton>
                  <LiquidMetalButton
                    onClick$={saveDiagnostics}
                    class="px-4 py-2 text-sm"
                  >
                    {diagBusy.value ? "Gathering…" : "Save diagnostic report"}
                  </LiquidMetalButton>
                  {diagSavedPath.value && !diagBusy.value && (
                    <button
                      type="button"
                      onClick$={revealDiagnostics}
                      class="text-sm text-[var(--text-link)] hover:underline"
                    >
                      Show in folder
                    </button>
                  )}
                </div>
                {diagCopied.value && !diagBusy.value && (
                  <p class="mt-3 text-xs text-[var(--text-muted)]">
                    Report copied - paste it into your reply.
                  </p>
                )}
                {diagSavedPath.value && !diagBusy.value && (
                  <p class="mt-3 text-xs text-[var(--text-muted)]">
                    Saved to {diagSavedPath.value} - attach this file when
                    asking for help.
                  </p>
                )}
                {diagError.value && (
                  <p class="mt-3 text-xs text-red-400">{diagError.value}</p>
                )}
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
