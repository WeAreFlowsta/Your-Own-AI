/**
 * Per-conversation export — a shareable, verifiable "receipt" of a conversation
 * (distinct from the Vault key-bearing backup: this is proof you can hand to
 * others, never your keys).
 *
 * Renders the conversation + its provenance to self-contained Markdown: messages,
 * each entry's Holochain action hash (the integrity anchor), the AI's agent key,
 * web sources, and — the differentiator — the GROUNDED sources (each claim tied to
 * its verbatim quote + the source document's SHA-256). Sensitive/verbose pieces
 * (the AI's system prompt, full attachment contents, the AI's reasoning) are OFF by
 * default and opt-in, since this is for sharing. Saved to Downloads via
 * `save_text_download`. Returns the saved path.
 *
 * Deterministic by design so it can later be signed via Sign It and verified on
 * the Sign It network.
 */
import { invoke } from "@tauri-apps/api/core";
import type { HolochainConversation, HolochainTranscriptEntry } from "../types";

/** What to disclose. All default OFF — safe to share. */
export interface ExportOptions {
  includeSystemPrompt?: boolean;
  includeAttachmentContent?: boolean;
  includeThinking?: boolean;
}

function fmt(ts: number): string {
  // Holochain timestamps are microseconds.
  const ms = ts > 1e15 ? ts / 1000 : ts;
  return new Date(ms).toLocaleString();
}

function slug(s: string, max = 40): string {
  return (
    s
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, max) || "conversation"
  );
}

export function conversationToMarkdown(
  conv: HolochainConversation,
  entries: HolochainTranscriptEntry[],
  aiName: string,
  options: ExportOptions = {},
  signed = false,
): string {
  const out: string[] = [];
  out.push(`# Conversation with ${aiName}`);
  out.push("");
  out.push(`- **Started:** ${fmt(conv.started_at)}`);
  out.push(`- **Model:** ${conv.model_used}`);
  out.push(`- **Messages:** ${entries.length}`);
  out.push(`- **Conversation hash:** \`${conv.hash}\``);
  if (conv.agent_key) {
    out.push(`- **AI agent key:** \`${conv.agent_key}\``);
  }
  out.push(
    `- _Tamper-evident record on ${aiName}'s Holochain source chain — every entry below is content-addressed and signed by the agent above; the hash is its verification anchor._`,
  );
  out.push("");

  for (const e of entries) {
    const who = e.role === "user" ? "You" : aiName;
    out.push("---");
    out.push("");
    out.push(`## ${who} · ${fmt(e.timestamp)}`);
    out.push("");
    out.push(e.content || "");
    out.push("");
    out.push(`\`entry ${e.hash}\``);
    out.push("");

    if (e.role === "user" && e.attachments) {
      out.push(
        `> **Attached document** — ${(e.attachments.bytes / 1024).toFixed(0)} KB · sha256 \`${e.attachments.sha256}\``,
      );
      out.push("");
      if (options.includeAttachmentContent && e.attachments.content) {
        out.push("```");
        out.push(e.attachments.content);
        out.push("```");
        out.push("");
      } else {
        out.push("> _Hash recorded for verification; content withheld from this export._");
        out.push("");
      }
    }

    if (e.role === "user" && e.images && e.images.length > 0) {
      for (const img of e.images) {
        out.push(
          `> **Attached image** — ${img.filename} · ${img.mime} · ${(img.bytes / 1024).toFixed(0)} KB · sha256 \`${img.sha256}\``,
        );
        out.push("");
        if (options.includeAttachmentContent && img.content) {
          out.push(`![${img.filename}](${img.content})`);
          out.push("");
        }
      }
    }

    if (e.role === "assistant") {
      const meta: string[] = [`model \`${e.model}\``];
      if (e.routing_reason) meta.push(`routed: ${e.routing_reason}`);
      if (e.runtime) {
        meta.push(e.runtime.online ? "online" : "local");
        meta.push(`app ${e.runtime.app_version}`);
        if (e.runtime.max_tokens != null) meta.push(`max ${e.runtime.max_tokens} tok`);
      }
      if (e.mode) meta.push(`mode ${e.mode}`);
      if (e.tokens) meta.push(`${e.tokens.total_tokens} tokens`);
      out.push(`*${meta.join(" · ")}*`);
      out.push("");

      if (options.includeSystemPrompt && e.system_prompt) {
        out.push("<details><summary>System prompt</summary>");
        out.push("");
        out.push("```");
        out.push(e.system_prompt);
        out.push("```");
        out.push("</details>");
        out.push("");
      }
      if (options.includeThinking && e.thinking) {
        out.push("<details><summary>Reasoning</summary>");
        out.push("");
        out.push(e.thinking);
        out.push("");
        out.push("</details>");
        out.push("");
      }
      if (e.sources && e.sources.length > 0) {
        out.push("**Web sources**");
        out.push("");
        for (const s of e.sources) {
          out.push(`- [${s.title || s.url}](${s.url})`);
        }
        out.push("");
      }
      if (e.grounded && e.grounded.length > 0) {
        out.push("**Grounded sources** — claims tied to the document that backs them");
        out.push("");
        for (const g of e.grounded) {
          if (g.kind === "document") {
            if (g.claim) out.push(`- ${g.claim}`);
            if (g.quote) out.push(`  > “${g.quote}”`);
            const where = g.span ? `chars ${g.span[0]}–${g.span[1]}` : "located by content";
            out.push(`  _${g.doc_name || "document"} · ${where} · sha256 \`${g.doc_sha256}\`_`);
          } else {
            out.push(`- 🖼 ${g.doc_name || "image"} · sha256 \`${g.doc_sha256}\``);
          }
          out.push("");
        }
      }
    }
  }

  out.push("---");
  out.push("");
  out.push(
    signed
      ? "_This receipt proves the conversation was recorded immutably on Holochain at the times shown. This exact file is signed with the owner's Flowsta identity on the Sign It network - verify it anytime at flowsta.com/sign-it._"
      : "_This receipt proves the conversation was recorded immutably on Holochain at the times shown. Independent verification of the content against the encrypted chain (without the owner's key) is coming via Sign It._",
  );
  out.push("");

  return out.join("\n");
}

/** SHA-256 of the exact export text, hex - what Sign It signs and verifies. */
async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export interface ExportResult {
  path: string;
  signed: boolean;
  /** Hex action hash of the published Sign It record, when signed. */
  actionHash?: string;
  /** Set when signing was requested but failed - the file saved UNSIGNED. */
  signError?: string;
}

/** Map a vault_sign_document error ("code" or "code|description") to a short
 *  human explanation for the export status line. */
export function describeSignError(e: string): string {
  const [code, description] = String(e).split("|", 2);
  if (description) return description;
  switch (code) {
    case "vault_not_found":
      return "Flowsta Vault isn't running.";
    case "vault_locked":
      return "Your Flowsta Vault is locked - unlock it and try again.";
    case "vault_outdated":
      return "Your Flowsta Vault needs an update before apps can publish signatures.";
    case "user_denied":
      return "Signing was declined in the Vault.";
    case "quota_exceeded":
      return "You've used all your signatures for this period.";
    case "sign_timeout":
      return "The Vault didn't finish in time - try again.";
    default:
      return code;
  }
}

/**
 * Build the Markdown and save it to Downloads. With `sign`, the exact file
 * is first signed + published to the Sign It network via the user's Vault
 * (approval dialog there; uses one signature from their plan) - the file is
 * hashed BEFORE saving and never modified after, so anyone can verify the
 * saved file by its hash. If signing fails, the export still saves, unsigned
 * (with the honest footer), and the failure is reported in the result.
 */
export async function exportConversation(
  conv: HolochainConversation,
  entries: HolochainTranscriptEntry[],
  aiName: string,
  options: ExportOptions = {},
  sign = false,
): Promise<ExportResult> {
  const ms = conv.started_at > 1e15 ? conv.started_at / 1000 : conv.started_at;
  const datePart = new Date(ms).toISOString().slice(0, 10);
  const filename = `yoai-${slug(aiName, 24)}-${slug(conv.title || "conversation")}-${datePart}.md`;

  if (sign) {
    const md = conversationToMarkdown(conv, entries, aiName, options, true);
    try {
      const res = await invoke<{ action_hash?: string | null }>("vault_sign_document", {
        fileHashHex: await sha256Hex(md),
        label: filename,
        comment: `Your Own AI conversation receipt - ${aiName}`,
      });
      const path = await invoke<string>("save_text_download", { filename, content: md });
      return { path, signed: true, actionHash: res?.action_hash ?? undefined };
    } catch (e) {
      const unsigned = conversationToMarkdown(conv, entries, aiName, options, false);
      const path = await invoke<string>("save_text_download", { filename, content: unsigned });
      return { path, signed: false, signError: String(e) };
    }
  }

  const md = conversationToMarkdown(conv, entries, aiName, options);
  const path = await invoke<string>("save_text_download", { filename, content: md });
  return { path, signed: false };
}
