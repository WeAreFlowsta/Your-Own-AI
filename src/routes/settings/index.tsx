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

/** The page's section order — drives both the layout and the jump nav. */
const SECTIONS = [
  { id: "settings-account", label: "Flowsta Account" },
  { id: "settings-backups", label: "Backups & recovery" },
  { id: "settings-behavior", label: "AI behavior" },
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

export default component$(() => {
  const nav = useNavigate();

  const showModelWidget = useSignal(false);
  const showHelpTips = useSignal(true);
  const allowAttachmentsOnline = useSignal(false);
  const groundDocumentsAuto = useSignal(false);
  const smartModeDetection = useSignal(true);
  const currentModel = useSignal<string | null>(null);
  const routingEagerness = useSignal<"privacy" | "balanced" | "freshness">("balanced");
  const routingLean = useSignal<"speed" | "balanced" | "quality">("balanced");
  const routingEscalateHard = useSignal(false);
  // May online routing options be OFFERED? (signed in + plan; starts true so
  // a slow check never hides controls from a paying user.)
  const onlineEntitled = useSignal(true);

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
    currentModel.value = localStorage.getItem("currentModel");
    const savedEagerness = localStorage.getItem("smartRoutingEagerness");
    if (savedEagerness === "privacy" || savedEagerness === "freshness") {
      routingEagerness.value = savedEagerness;
    }
    const savedLean = localStorage.getItem("routingOfflineLean");
    if (savedLean === "speed" || savedLean === "quality") {
      routingLean.value = savedLean;
    }
    // One-time migration: hard-question escalation used to be implied by
    // freshness eagerness — existing freshness users keep their behavior
    // and see the toggle honestly ON.
    if (
      localStorage.getItem("routingEscalateHard") === null &&
      savedEagerness === "freshness"
    ) {
      localStorage.setItem("routingEscalateHard", "true");
    }
    routingEscalateHard.value =
      localStorage.getItem("routingEscalateHard") === "true";

    import("../../utils/entitlement")
      .then(({ getOnlineEntitlement }) => getOnlineEntitlement())
      .then((e) => (onlineEntitled.value = e.entitled))
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

  const toggleSmartMode = $(() => {
    smartModeDetection.value = !smartModeDetection.value;
    localStorage.setItem("smartModeDetection", smartModeDetection.value.toString());
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

  const toggleEscalateHard = $(() => {
    routingEscalateHard.value = !routingEscalateHard.value;
    localStorage.setItem(
      "routingEscalateHard",
      routingEscalateHard.value.toString()
    );
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

                {/* Offline model choice — biases the fit-aware offline pick. */}
                <h3 class="text-lg font-semibold text-[var(--text-primary)]">
                  Offline model choice
                </h3>
                <p class="text-sm text-[var(--text-secondary)] mt-1 mb-3">
                  Which of your downloaded models the Auto modes lean toward.
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
                {/* Online eagerness — unchanged shipped knob. */}
                <h3 class="text-lg font-semibold text-[var(--text-primary)]">
                  Going online for fresh information
                </h3>
                <p class="text-sm text-[var(--text-secondary)] mt-1 mb-3">
                  For AIs set to "Auto — Online and Offline": how eagerly a question
                  that may need up-to-date information goes to an online model.
                  "Offline Only" AIs never go online.
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

                {/* Hard-question escalation — rides Router V2 difficulty scoring.
                    Independent of the freshness picker above by design: one
                    axis = fresh info, this axis = difficulty. */}
                <SettingToggle
                  title="Hand hard questions to a stronger model"
                  checked={routingEscalateHard}
                  onToggle$={toggleEscalateHard}
                >
                  When a question looks genuinely difficult — tricky code, deep
                  reasoning, complex math — your AI may pass it to a stronger
                  online model instead of answering with your offline one. Off
                  means hard questions stay on your device like everything else.
                  Only affects AIs set to "Auto — Online and Offline".
                </SettingToggle>
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
        v{packageJson.version}
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Settings - Your Own AI",
};
