/**
 * Settings > Storage: where models and components live, and what is
 * installed on this computer (remove to reclaim space). Getting more lives
 * in Add-ons > Components - the same cards, in "get" mode.
 */
import { component$ } from "@builder.io/qwik";
import ModelStorageLocation from "./ModelStorageLocation";
import { ComponentCard, BuildComponentCard } from "./ComponentCards";
import { EMBEDDING_MODEL, UTILITY_MODEL, VISION_PROJECTORS, OCR_MODELS } from "../data/recommended-models";

export default component$(() => {
  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]">
      <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-1">
        Storage
      </h2>
      <p class="text-sm text-[var(--text-secondary)] mb-4">
        Where models and components live, and what is installed on this computer. Remove any to
        reclaim space. To get more - engines, vision, OCR, Projects, the programs tools need - go to{' '}
        <a href="/add-ons/components" class="text-[var(--text-link)] hover:underline">Add-ons › Components</a>.
      </p>
      <h3 class="font-semibold text-[var(--text-primary)] mb-1">Storage location</h3>
      <p class="text-sm text-[var(--text-secondary)] mb-3">
        Models and the other large components download to one folder. Move it to any drive - an
        external SSD, a roomier partition - and future downloads follow.
      </p>
      <ModelStorageLocation showPath />
      <div class="mt-6 pt-5 border-t border-[var(--border-subtle)]">
        <h3 class="font-semibold text-[var(--text-primary)] mb-3">Installed components</h3>
        <div class="flex flex-col gap-3">
          <BuildComponentCard manageOnly />
          <ComponentCard manageOnly model={EMBEDDING_MODEL} icon="brain" activeLabel="Installed - memory and smart routing are active." stopCommand="stop_embedding_server" />
          <ComponentCard manageOnly model={UTILITY_MODEL} icon="spark" activeLabel="Installed - extraction and smart modes run on your device, even with online AIs." stopCommand="stop_utility_server" />
          {VISION_PROJECTORS.map((projector) => (
            <ComponentCard manageOnly key={projector.id} model={projector} icon="eye" activeLabel="Installed - this model can now see images you attach." />
          ))}
          <ComponentCard manageOnly model={OCR_MODELS} icon="scan" activeLabel="Installed - scanned PDFs are read on your device." />
        </div>
        <p class="mt-3 text-xs text-[var(--text-muted)]">Engines (CUDA, MLX) are managed under Engines below.</p>
      </div>
    </section>
  );
});
