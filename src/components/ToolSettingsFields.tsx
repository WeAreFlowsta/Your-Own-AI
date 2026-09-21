import { component$ } from '@builder.io/qwik';
import type { McpServer } from '../utils/mcp';

export interface ToolSettingsFieldsProps {
  config: NonNullable<McpServer['config']>;
  /** A store the fields write into: key -> value (toggles hold on / off). */
  draft: Record<string, string>;
  /** Which settings already hold a value (secrets never come back). */
  filled: Record<string, boolean>;
}

/** A tool's settings as form fields - the same fields in the Set up list and
 *  in the settings dialog. The caller owns the draft and the saving. */
export const ToolSettingsFields = component$<ToolSettingsFieldsProps>(({ config, draft, filled }) => (
  <div class="space-y-3">
    {config.map((f) => {
      const key = f.key;
      // The card says when a setting does not apply (another switch is on).
      const idle = !!f.unless && (draft[f.unless] ?? 'off') === 'on';
      if (f.kind === 'toggle') {
        return (
          <label key={key} class={`flex items-start gap-2 text-xs text-[var(--text-secondary)] ${idle ? 'opacity-40' : 'cursor-pointer'}`}>
            <input
              type="checkbox"
              disabled={idle}
              class="mt-0.5 cursor-pointer"
              checked={(draft[key] ?? 'off') === 'on'}
              onChange$={(_, el) => { draft[key] = el.checked ? 'on' : 'off'; }}
            />
            <span>{f.label || f.key}</span>
          </label>
        );
      }
      return (
        <label key={key} class={`block text-xs text-[var(--text-secondary)] ${idle ? 'opacity-40' : ''}`}>
          {f.label || f.key}{f.required ? '' : ' (optional)'}
          {f.kind === 'secret' && filled[key] && <span class="ml-2 text-emerald-500">set - leave blank to keep</span>}
          <input
            type={f.kind === 'secret' ? 'password' : 'text'}
            value={draft[key] ?? ''}
            onInput$={(_, el) => { draft[key] = el.value; }}
            placeholder={f.hint ?? ''}
            disabled={idle}
            autocomplete="off"
            class="mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none"
          />
          {f.kind === 'path' && !idle && (
            <button
              type="button"
              class="mt-1 text-[var(--text-link)] hover:underline cursor-pointer"
              onClick$={async () => {
                const { open } = await import('@tauri-apps/plugin-dialog');
                const picked = await open({ directory: true, multiple: false });
                if (picked && !Array.isArray(picked)) draft[key] = picked;
              }}
            >
              Choose the folder
            </button>
          )}
        </label>
      );
    })}
  </div>
));
