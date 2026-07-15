/**
 * Shared markdown rendering utility.
 *
 * Renders markdown text to styled HTML using marked + highlight.js.
 * Used by ChatMessage, Memory page, and thinking panels.
 */
import { marked } from "marked";
import { markedHighlight } from "marked-highlight";
import hljs from "highlight.js";

// Configure marked with highlight.js (runs once on import)
marked.use({ gfm: true, breaks: false });
marked.use(
  markedHighlight({
    emptyLangClass: "hljs",
    langPrefix: "hljs language-",
    highlight(code: string, lang: string) {
      if (lang && hljs.getLanguage(lang)) {
        return hljs.highlight(code, { language: lang }).value;
      }
      return hljs.highlightAuto(code).value;
    },
  }),
);

/**
 * Render markdown text to styled HTML string.
 */
export function renderMarkdown(text: string): string {
  let html = marked.parse(text) as string;
  // Add styled classes to elements
  html = html
    .replace(/<p>/g, '<p class="mb-3 last:mb-0">')
    .replace(
      /<ol>/g,
      '<ol class="list-decimal list-inside my-2 ml-4 space-y-1">',
    )
    .replace(/<ul>/g, '<ul class="list-disc list-inside my-2 ml-4 space-y-1">')
    .replace(/<li>/g, '<li class="py-1">')
    .replace(/<strong>/g, '<strong class="font-bold">')
    .replace(/<h1>/g, '<h1 class="text-xl font-semibold my-3">')
    .replace(/<h2>/g, '<h2 class="text-lg font-semibold my-3">')
    .replace(/<h3>/g, '<h3 class="text-base font-semibold my-3">')
    .replace(
      /<table>/g,
      '<table class="w-full my-4 text-sm border-collapse">',
    )
    .replace(
      /<tr>/g,
      '<tr class="border-b border-[var(--border-subtle)]">',
    )
    .replace(
      /<th>/g,
      '<th class="p-2 font-semibold text-left border-b-2 border-[var(--border-primary)]">',
    )
    .replace(/<td>/g, '<td class="p-2 align-top">');
  return html;
}
