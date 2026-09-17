import { component$, useSignal, useVisibleTask$ } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import { Callout } from './Callout';

/**
 * Danger notice: the operating system will not run the AI engine on this
 * machine (the engine needs a newer OS release than the one installed), so
 * no offline model can load. Said up front - on the start view and above
 * the Offline Models list - so nobody downloads a model and then meets a
 * load that can never succeed. Not dismissible: nothing offline works until
 * the OS or the app changes.
 */
export default component$<{ class?: string }>((props) => {
  const blocked = useSignal(false);
  const isMac = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const ok = await invoke<boolean>('engine_start_check');
      blocked.value = !ok;
      isMac.value = /Mac/i.test(navigator.userAgent);
    } catch {
      /* stays hidden */
    }
  });

  if (!blocked.value) return null;

  return (
    <Callout
      intent="danger"
      title="Offline models can't start on this computer yet"
      class={props.class ?? 'mt-10 text-left'}
    >
      <p class="mb-2.5">
        {isMac.value
          ? 'The AI engine in this version of Your Own AI needs a newer macOS than this Mac is running, so offline models will not load. Your Mac and your models are fine.'
          : 'The AI engine in this version of Your Own AI needs a newer release of your operating system than this computer is running, so offline models will not load. Your computer and your models are fine.'}
      </p>
      <p>
        Updating Your Own AI when a new version is out, or updating your
        operating system, fixes it. Until then,{' '}
        <a href="/online-models" class="text-[var(--text-link)] hover:underline">
          online models
        </a>{' '}
        work as normal.
      </p>
    </Callout>
  );
});
