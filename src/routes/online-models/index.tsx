import { loadedModelNow } from "../../utils/loadedModel";
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
import { useHeaderWorkspace } from "../../hooks/useHeaderWorkspace";

export default component$(() => {
  const nav = useNavigate();
  const headerWs = useHeaderWorkspace();
  const currentModel = useSignal<string | null>(null);
  const showModelWidget = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    loadedModelNow().then((m) => { currentModel.value = m; });
    showModelWidget.value = localStorage.getItem("showModelWidget") === "true";

    const sync = () => {
      loadedModelNow().then((m) => { currentModel.value = m; });
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
      {/* z-20: above page content so header dropdowns (projects, menus)
          are never painted over by in-page stacking contexts like the
          AI selector row (z-10). Page modals are fixed z-50 and still
          cover the header. */}
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
