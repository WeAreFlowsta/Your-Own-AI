import { component$ } from "@builder.io/qwik";
import {
  QwikCityProvider,
  RouterOutlet,
} from "@builder.io/qwik-city";

import "./index.css";

export default component$(() => {
  return (
    <QwikCityProvider>
      <head>
        <meta charset="utf-8" />
        <link rel="icon" type="image/svg+xml" href="/vite.svg" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <title>Your Own AI</title>
        {/* Prevent FOUC: apply theme + background synchronously */}
        <script
          dangerouslySetInnerHTML={`
            (function(){
              var t = localStorage.getItem('theme') || 'dark';
              if (t !== 'light' && t !== 'dark') t = 'dark';
              var bg = {dark:'#000000',light:'#fafafc'}[t];
              document.documentElement.classList.add('theme-' + t);
              document.documentElement.style.backgroundColor = bg;
            })();
          `}
        />
        {/* Loading overlay animation */}
        <style dangerouslySetInnerHTML="@keyframes app-spin{to{transform:rotate(360deg)}}" />
      </head>
      <body lang="en" class="theme-dark" style="background-color:#000000">
        <div
          id="app-loading"
          style="position:fixed;inset:0;z-index:99999;display:flex;align-items:center;justify-content:center;background:#000000;transition:opacity 0.3s ease"
        >
          <div style="display:flex;flex-direction:column;align-items:center">
            <svg width="48" height="48" viewBox="0 0 24 24" fill="none" style="animation:app-spin 1s linear infinite">
              <circle cx="12" cy="12" r="10" stroke="#4e5cde" stroke-width="2.5" stroke-dasharray="31.4 31.4" stroke-linecap="round" />
            </svg>
            <div id="app-loading-text" style="margin-top:16px;color:#888;font-family:'Plus Jakarta Sans',system-ui,sans-serif;font-size:14px">
              Loading your AIs...
            </div>
            {/* Startup-failure escape hatch: if the app never finishes
                loading (e.g. antivirus killing the records engine, as on
                MUMATİ's machine), this reveals a way to save a diagnostic
                report WITHOUT reaching Settings. Pure vanilla + the global
                Tauri API, so nothing about it depends on the frozen app. */}
            <div id="app-loading-help" style="display:none;margin-top:28px;max-width:340px;text-align:center;font-family:'Plus Jakarta Sans',system-ui,sans-serif">
              <div style="color:#aaa;font-size:13px;line-height:1.5">
                This is taking longer than usual. Some antivirus tools block
                Your Own AI from starting. You can save a diagnostic report to
                send to support - it works even when the app won't open.
              </div>
              <button id="app-loading-diag-btn" style="margin-top:14px;padding:8px 18px;border:1px solid #4e5cde;border-radius:9px;background:transparent;color:#c7ccf5;font-size:13px;cursor:pointer">
                Save a diagnostic report
              </button>
              <button id="app-loading-copy-btn" style="margin-top:10px;margin-left:8px;padding:8px 18px;border:1px solid #555;border-radius:9px;background:transparent;color:#aaa;font-size:13px;cursor:pointer">
                Copy to clipboard
              </button>
              <div id="app-loading-diag-result" style="margin-top:12px;color:#888;font-size:12px;line-height:1.5;word-break:break-all"></div>
            </div>
          </div>
        </div>
        <script
          dangerouslySetInnerHTML={`
            (function(){
              var t = localStorage.getItem('theme') || 'dark';
              var bg = {dark:'#000000',light:'#fafafc'}[t];
              var overlay = document.getElementById('app-loading');
              if (overlay) overlay.style.background = bg;

              // Reveal the diagnostics escape hatch if the overlay is still
              // up after 60s - i.e. startup never completed. AiDataContext
              // hides the whole overlay on success, so a visible overlay
              // here means the app is genuinely stuck. 60s clears even a
              // slow-but-healthy first launch (MUMATİ's never converges, so
              // the exact threshold only matters for false positives). It's
              // additive anyway - a late success still hides the whole thing.
              setTimeout(function(){
                var ov = document.getElementById('app-loading');
                if (!ov || ov.style.display === 'none' || ov.style.opacity === '0') return;
                var help = document.getElementById('app-loading-help');
                if (help) help.style.display = 'block';
              }, 60000);

              var btn = document.getElementById('app-loading-diag-btn');
              if (btn) btn.addEventListener('click', function(){
                var out = document.getElementById('app-loading-diag-result');
                btn.disabled = true;
                btn.textContent = 'Saving...';
                try {
                  window.__TAURI__.core.invoke('export_diagnostics', { path: '' })
                    .then(function(p){
                      if (out) out.textContent = 'Saved to: ' + p;
                      btn.textContent = 'Saved';
                    })
                    .catch(function(e){
                      if (out) out.textContent = 'Could not save: ' + e;
                      btn.disabled = false;
                      btn.textContent = 'Try again';
                    });
                } catch (e) {
                  if (out) out.textContent = 'Could not save: ' + e;
                  btn.disabled = false;
                  btn.textContent = 'Try again';
                }
              });

              // Clipboard variant of the same report - for users who find
              // locating a Desktop file harder than pasting into a chat.
              var copyBtn = document.getElementById('app-loading-copy-btn');
              if (copyBtn) copyBtn.addEventListener('click', function(){
                var out = document.getElementById('app-loading-diag-result');
                copyBtn.disabled = true;
                copyBtn.textContent = 'Copying...';
                try {
                  window.__TAURI__.core.invoke('copy_diagnostics')
                    .then(function(){
                      if (out) out.textContent = 'Copied - paste it into your message to support.';
                      copyBtn.textContent = 'Copied';
                    })
                    .catch(function(e){
                      if (out) out.textContent = 'Could not copy: ' + e;
                      copyBtn.disabled = false;
                      copyBtn.textContent = 'Try again';
                    });
                } catch (e) {
                  if (out) out.textContent = 'Could not copy: ' + e;
                  copyBtn.disabled = false;
                  copyBtn.textContent = 'Try again';
                }
              });
            })();
          `}
        />
        <RouterOutlet />
      </body>
    </QwikCityProvider>
  );
});
