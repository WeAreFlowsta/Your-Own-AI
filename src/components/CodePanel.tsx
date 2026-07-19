import { component$, useSignal, useVisibleTask$, $, type QRL } from '@builder.io/qwik';
import hljs from 'highlight.js';
import { LuX, LuEye, LuCode, LuCopy, LuCheck, LuTerminal } from '@qwikest/icons/lucide';

/** Languages whose blocks are commands, not code - they earn the
 *  open-in-terminal button. */
export const SHELL_LANGUAGES = ['bash', 'sh', 'shell', 'zsh', 'console', 'terminal', 'cmd', 'bat', 'powershell'];

/** Copyable text of a command block: strip "$ "/"> " prompts when every
 *  non-empty line carries one - pasting prompts breaks the command. */
export function commandText(code: string): string {
  const lines = code.split('\n');
  const prompted = lines.filter((l) => l.trim() !== '');
  if (prompted.length > 0 && prompted.every((l) => /^\s*[$>]\s/.test(l))) {
    return lines.map((l) => l.replace(/^\s*[$>]\s?/, '')).join('\n');
  }
  return code;
}

interface CodePanelProps {
  codeString: string;
  language: string;
  onClose$?: () => void;
  isOverlay?: boolean;
  theme: 'light' | 'dark';
  /** Run this block in the user's own terminal (shell languages only). */
  onOpenTerminal$?: QRL<(command: string) => void>;
}

export const CodePanel = component$<CodePanelProps>(({ codeString, language, onClose$, isOverlay, theme, onOpenTerminal$ }) => {
  const isRenderView = useSignal(false);
  const codeRef = useSignal<HTMLElement>();
  const copied = useSignal(false);

  const copyCode = $(async () => {
    try {
      await navigator.clipboard.writeText(commandText(codeString));
      copied.value = true;
      setTimeout(() => (copied.value = false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  });

  const canRender = language && ['html', 'javascript', 'css', 'svg', 'xml', 'markup'].includes(language.toLowerCase());
  const isShell = !!language && SHELL_LANGUAGES.includes(language.toLowerCase());

  // SNYK SEC-FIX: Ensure the sandbox attribute is comprehensive to prevent exploits.
  const sandboxPermissions = "allow-scripts";

  const createIframeContent = () => {
    if (language === 'html' || language === 'svg' || language === 'xml' || language === 'markup') {
      return codeString;
    }
    if (language === 'javascript') {
      return `<script>${codeString}<\/script>`;
    }
    if (language === 'css') {
      return `<style>${codeString}<\/style>`;
    }
    return '';
  };

  // Use highlight.js to highlight code blocks when not in render view
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    track(() => isRenderView.value);
    track(() => codeString);
    if (!isRenderView.value && codeRef.value) {
      // Reset previous highlighting
      codeRef.value.removeAttribute('data-highlighted');
      hljs.highlightElement(codeRef.value);
    }
  });

  return (
    <div class={`code-panel-container theme-${theme}`}>
      <div class="code-panel-header">
        <span class="code-panel-language">{language || 'code'}</span>
        <div class="code-panel-actions">
          <button
            onClick$={copyCode}
            class="code-panel-toggle"
            title={copied.value ? 'Copied' : 'Copy'}
          >
            <LuCheck style={{ width: '18px', height: '18px', display: copied.value ? undefined : 'none' }} />
            <LuCopy style={{ width: '18px', height: '18px', display: copied.value ? 'none' : undefined }} />
          </button>
          {isShell && onOpenTerminal$ && (
            <button
              onClick$={() => onOpenTerminal$(commandText(codeString))}
              class="code-panel-toggle"
              title="Run in your terminal"
            >
              <LuTerminal style={{ width: '18px', height: '18px' }} />
            </button>
          )}
          {canRender && (
            <div class="code-panel-toggle-group">
              <button
                onClick$={() => { isRenderView.value = false; }}
                class={`code-panel-toggle ${!isRenderView.value ? 'active' : ''}`}
                title="View Code"
              >
                <LuCode style={{ width: '18px', height: '18px' }} />
              </button>
              <button
                onClick$={() => { isRenderView.value = true; }}
                class={`code-panel-toggle ${isRenderView.value ? 'active' : ''}`}
                title="Render Preview"
              >
                <LuEye style={{ width: '18px', height: '18px' }} />
              </button>
            </div>
          )}
          {onClose$ && (
            <button onClick$={onClose$} class="code-panel-close">
              <LuX style={{ width: '20px', height: '20px' }} />
            </button>
          )}
        </div>
      </div>
      <div class="code-panel-content">
        {isRenderView.value ? (
          <iframe
            srcdoc={createIframeContent()}
            title="Code Preview"
            sandbox={sandboxPermissions}
            class="w-full h-full border-0"
            style={{ backgroundColor: '#fff' }}
          />
        ) : (
          <pre
            style={{
              margin: '0',
              padding: '1rem',
              backgroundColor: 'var(--bg-code-block)',
              height: '100%',
              overflow: 'auto',
            }}
          >
            <code
              ref={codeRef}
              class={language ? `language-${language}` : ''}
              style={{ fontFamily: 'var(--font-mono)' }}
            >
              {codeString}
            </code>
          </pre>
        )}
      </div>

      <style>{`
        .code-panel-container {
          position: ${isOverlay ? 'fixed' : 'relative'};
          top: 0;
          left: 0;
          width: 100%;
          height: 100%;
          background-color: var(--bg-main);
          z-index: ${isOverlay ? '100' : 'auto'};
          display: flex;
          flex-direction: column;
          border: 1px solid var(--border-subtle);
          overflow: hidden;
        }
        .code-panel-header {
          display: flex;
          justify-content: space-between;
          align-items: center;
          padding: 0.5rem 1rem;
          background-color: var(--bg-subtle);
          border-bottom: 1px solid var(--border-subtle);
        }
        .code-panel-language {
          font-family: var(--font-mono);
          font-size: 0.875rem;
          color: var(--text-muted);
        }
        .code-panel-actions {
          display: flex;
          align-items: center;
          gap: 1rem;
        }
        .code-panel-toggle-group {
          display: flex;
          border-radius: 9999px;
          border: 1px solid var(--border-input);
          overflow: hidden;
          box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05);
        }
        .code-panel-toggle {
          padding: 0.375rem 0.75rem;
          font-size: 0.875rem;
          color: var(--text-secondary);
          background-color: transparent;
          border: none;
          cursor: pointer;
          display: flex;
          align-items: center;
          justify-content: center;
          transition: background-color 0.2s, color 0.2s;
          border-left: 1px solid var(--border-input);
        }
        .code-panel-toggle-group button:first-child {
          border-left: none;
        }
        .code-panel-toggle:hover:not(.active) {
          background-color: var(--bg-hover);
        }
        .code-panel-toggle.active {
          background-color: var(--bg-button-primary);
          color: var(--text-button-primary);
        }
        .code-panel-close {
          color: var(--text-secondary);
          background-color: transparent;
          border: none;
          cursor: pointer;
        }
        .code-panel-content {
          flex-grow: 1;
          overflow: auto;
          background-color: var(--bg-code-block);
        }
      `}</style>
    </div>
  );
});

export default CodePanel;
