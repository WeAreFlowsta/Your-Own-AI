import { component$, useSignal, $, type QRL } from '@builder.io/qwik';
import { LuSparkles, LuPaperclip, LuFileText, LuCode } from '@qwikest/icons/lucide';
import type { ChatAction } from '../types';

interface ActionMenuProps {
  setSelectedAction$: QRL<(action: ChatAction) => void>;
  onAttachFiles$: QRL<(paths: string[]) => void>;
  isMultiLine: boolean;
}


export const ActionMenu = component$<ActionMenuProps>((props) => {
  const actionMenuOpen = useSignal(false);
  const buttonRef = useSignal<HTMLElement>();
  const menuPos = useSignal({ left: 0, bottom: 0 });

  const handleActionSelection = $((action: ChatAction) => {
    props.setSelectedAction$(action);
    actionMenuOpen.value = false;
  });

  const handleAttachFile = $(async () => {
    actionMenuOpen.value = false;
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        multiple: true,
        filters: [{
          name: 'Documents, Code & Images',
          extensions: [
            'txt','md','csv','json','xml','yaml','yml','toml','log','ini','cfg','conf','env',
            'py','js','ts','tsx','jsx','rs','go','java','c','cpp','h','hpp','cs','rb','php','swift','kt',
            'css','scss','sass','less','html','htm','sql','sh','bash','zsh','ps1','bat',
            'r','m','jl','lua','pl','pm','ex','exs','hs','ml','fs','dart','vue','svelte',
            'pdf','docx','doc','xlsx','xls','ods','odt','rtf',
            'png','jpg','jpeg','gif','bmp',
          ],
        }],
      });
      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        props.onAttachFiles$(paths);
      }
    } catch (err) {
      console.error('[ActionMenu] File picker error:', err);
    }
  });

  return (
    <div class={`flex-shrink-0 transition-all duration-200 ${props.isMultiLine ? 'self-start mt-1' : 'self-center'}`}>
      <div class="relative inline-block text-left">
        <button
          ref={buttonRef}
          type="button"
          onClick$={() => {
            if (!actionMenuOpen.value && buttonRef.value) {
              const rect = buttonRef.value.getBoundingClientRect();
              menuPos.value = { left: rect.left, bottom: window.innerHeight - rect.top + 8 };
            }
            actionMenuOpen.value = !actionMenuOpen.value;
          }}
          class="flex items-center justify-center h-9 w-9 sm:h-10 sm:w-10 rounded-full text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors duration-200"
        >
          <LuSparkles class="h-5 w-5" />
        </button>
        {actionMenuOpen.value && (
          <>
            <div class="fixed inset-0 z-[45]" onClick$={() => { actionMenuOpen.value = false; }} />
            <div
              class="fixed w-56 rounded-2xl bg-[var(--bg-card)] py-1 shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none z-50 overflow-hidden"
              style={{ left: `${menuPos.value.left}px`, bottom: `${menuPos.value.bottom}px` }}
            >
              <button
                type="button"
                onClick$={() => handleActionSelection('Write a report...')}
                class="text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-dropdown-active)] group flex w-full items-center px-2 py-2 text-sm"
              >
                <LuFileText class="h-4 w-4 mr-3" />
                Write a report...
              </button>
              <button
                type="button"
                onClick$={() => handleActionSelection('Write code...')}
                class="text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-dropdown-active)] group flex w-full items-center px-2 py-2 text-sm"
              >
                <LuCode class="h-4 w-4 mr-3" />
                Write code...
              </button>
              <button
                type="button"
                onClick$={handleAttachFile}
                class="text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-dropdown-active)] group flex w-full items-center px-2 py-2 text-sm"
              >
                <LuPaperclip class="h-4 w-4 mr-3" />
                Add file
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
});
