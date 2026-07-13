/**
 * Online Models Page
 *
 * Frontier models via Flowsta. Sign-in gating + how-to when signed out;
 * the model catalog with pause/play when enabled. See components/OnlineModels.
 */

import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { OnlineModels } from "../../components/OnlineModels";
import AppHeader from "../../components/AppHeader";

export default component$(() => {
  const nav = useNavigate();
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    currentModel.value = localStorage.getItem("currentModel");
    showModelWidget.value = localStorage.getItem("showModelWidget") === "true";

    const sync = () => {
      currentModel.value = localStorage.getItem("currentModel");
      showModelWidget.value = localStorage.getItem("showModelWidget") === "true";
    };
    window.addEventListener("storage", sync);
    window.addEventListener("settingsChanged", sync);
    window.addEventListener("modelDeleted", sync);
    return () => {
      window.removeEventListener("storage", sync);
      window.removeEventListener("settingsChanged", sync);
      window.removeEventListener("modelDeleted", sync);
    };
  });

  const handleNewQuestion = $(() => {
    nav("/chat");
  });

  const handleModelsClick = $(() => {
    nav("/setup");
  });

  return (
    <div class="flex flex-col h-screen bg-[var(--bg-main)]">
      <div class="relative z-10">
        <AppHeader
          handleNewQuestion$={handleNewQuestion}
          handleModelsClick$={handleModelsClick}
          currentModel={currentModel.value}
          showModelWidget={showModelWidget.value && currentModel.value !== null}
        />
      </div>

      <div class="flex-1 overflow-y-auto">
        <div class="py-6">
          <OnlineModels />
        </div>
      </div>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Online Models - Your Own AI",
};
