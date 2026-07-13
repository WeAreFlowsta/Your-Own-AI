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
          </div>
        </div>
        <script
          dangerouslySetInnerHTML={`
            (function(){
              var t = localStorage.getItem('theme') || 'dark';
              var bg = {dark:'#000000',light:'#fafafc'}[t];
              document.getElementById('app-loading').style.background = bg;
            })();
          `}
        />
        <RouterOutlet />
      </body>
    </QwikCityProvider>
  );
});
