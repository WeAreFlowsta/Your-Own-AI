import { component$, useSignal, useVisibleTask$ } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import { Callout } from './Callout';
import OnlineModelsDoor from './OnlineModelsDoor';

interface EngineStartCheck {
  ok: boolean;
  reason?: 'loader' | 'macos-too-old' | null;
  needs_macos?: string | null;
  os_version?: string | null;
}

/**
 * Said up front - on the start view and above the Offline Models list - so
 * nobody downloads a model and then meets a load that cannot succeed.
 *
 * Two honest cases, neither dismissible (nothing changes until the OS or the
 * app does):
 *  - `loader`: the operating system will not run the engine at all. No
 *    offline model can load. Danger.
 *  - `macos-too-old`: the engine runs, but this macOS is older than it is
 *    supported on (Apple Silicon needs 13.3) and it can stop without warning.
 *    Warning - offline models are not blocked.
 */
export default component$<{ class?: string }>((props) => {
  const check = useSignal<EngineStartCheck | null>(null);
  const isMac = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      check.value = await invoke<EngineStartCheck>('engine_start_check');
      isMac.value = /Mac/i.test(navigator.userAgent);
    } catch {
      /* stays hidden */
    }
  });

  const c = check.value;
  if (!c || !c.reason) return null;
  const cls = props.class ?? 'mt-10 text-left';

  if (c.reason === 'macos-too-old') {
    return (
      <Callout intent="warning" title={`Offline models need macOS ${c.needs_macos ?? '13.3'} or later on this Mac`} class={cls}>
        <p class="mb-2.5">
          This Mac runs macOS {c.os_version ?? 'an older release'}. On Apple
          Silicon the AI engine is supported from macOS {c.needs_macos ?? '13.3'}.
          On this version an offline model can load and then stop without
          warning, part way through an answer. Your Mac and your models are fine.
          Updating macOS fixes it.
        </p>
        <OnlineModelsDoor />
      </Callout>
    );
  }

  return (
    <Callout intent="danger" title="Offline models can't start on this computer yet" class={cls}>
      <p class="mb-2.5">
        {isMac.value
          ? 'The AI engine in this version of Your Own AI needs a newer macOS than this Mac is running, so offline models will not load. Your Mac and your models are fine.'
          : 'The AI engine in this version of Your Own AI needs a newer release of your operating system than this computer is running, so offline models will not load. Your computer and your models are fine.'}{' '}
        Updating Your Own AI when a new version is out, or updating your
        operating system, fixes it.
      </p>
      <OnlineModelsDoor />
    </Callout>
  );
});
