import { component$, useSignal, useVisibleTask$ } from '@builder.io/qwik';
import hljs from 'highlight.js';
import { LuX, LuEye, LuCode } from '@qwikest/icons/lucide';

interface CodePanelProps {
  codeString: string;
  language: string;
  onClose$?: () => void;
  isOverlay?: boolean;
  theme: 'light' | 'dark';
}

export const CodePanel = component$<CodePanelProps>(({ codeString, language, onClose$, isOverlay, theme }) => {
  const isRenderView = useSignal(false);
  const codeRef = useSignal<HTMLElement>();

  const canRender = language && ['html', 'javascript', 'css', 'svg', 'xml', 'markup'].includes(language.toLowerCase());

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
            srcDoc={createIframeContent()}
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
