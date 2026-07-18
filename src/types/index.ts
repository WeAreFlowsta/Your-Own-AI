// Core type definitions for Desktop app

export interface Archetype {
  id: string;
  name: string;
  description: string;
  systemPromptTemplate: string;
  starterMessages: string[];
  thumbnailPath: string;
  category: string;
  tags: string[];
  defaultThumbnailUrl: string | null;
}

export interface UserDefinedAI {
  id: string;
  name: string;
  description: string;
  baseArchetypeId: string;
  model: string;  // e.g., "phi-3-mini-q4.gguf" (local model filename)
  systemPrompt: string;
  emoji?: string;  // Emoji to represent the AI (e.g., "🤖")
  askBlurb?: string;  // Custom text shown after AI name in input (e.g., "about marketing strategies")
  status: 'active' | 'inactive' | 'archived';  // 'archived' = retired but its signed transcripts + agent are kept
  lengthDisposition?: LengthDisposition;  // resting length lean (soft, not a cap)
  defaultMode?: TurnMode;  // the turn mode this AI defaults to (chat unless e.g. a report/coding AI)
  useEmojis?: boolean;  // Whether to use emojis in responses
  agentPubKey?: string;  // Holochain agent public key (set on first provisioning)
  archivedAt?: number;  // ms epoch when the AI was archived (set on archive, cleared on restore)
}

export interface CreateUserAiData {
  name: string;
  baseArchetypeId: string;
  systemPrompt: string;
  description: string;
  lengthDisposition: LengthDisposition;
  defaultMode?: TurnMode;
  model: string;
  emoji?: string;
  askBlurb?: string;
  useEmojis?: boolean;
}

export interface UpdateUserAiData {
  name?: string;
  baseArchetypeId?: string;
  systemPrompt?: string;
  description?: string;
  lengthDisposition?: LengthDisposition;
  defaultMode?: TurnMode;
  model?: string;
  emoji?: string;
  askBlurb?: string;
  useEmojis?: boolean;
}

/** The AI's resting length lean — a soft default, not a cap. */
export type LengthDisposition = 'conversational' | 'balanced' | 'thorough';

/** What kind of answer this one turn is. Drives the thinking/structure ceremony.
 *  Set by the sparkle action (explicit) → classifier (optional) → 'chat'. */
export type TurnMode = 'chat' | 'report' | 'code';

/** A selectable disposition in the Edit-AI dropdown. */
export interface ResponseLengthOption {
  id: LengthDisposition;
  name: string;
  description: string;
  maxTokens: number;  // generous ceiling so nothing truncates; length is shaped by prompt, not this
}

export interface LocalModel {
  name: string;
  size: string;
  modified_at: string;
  size_bytes: number;
  parameter_size: string;  // e.g., "3B", "7B"
  quantization: string;    // e.g., "Q4_0"
}

export type AppMode = 'local' | 'cloud';

/** An OpenAI multimodal content part — text or an image (as a data URL). */
export type ContentPart =
  | { type: 'text'; text: string }
  | { type: 'image_url'; image_url: { url: string } };

export interface ChatMessage {
  role: 'user' | 'assistant';
  /** Plain string for text turns; a content-part array for image (vision) turns. */
  content: string | ContentPart[];
}

/** An image attached to a chat turn, read as a base64 data URL for vision. */
export interface AttachedImage {
  filename: string;
  dataUrl: string;
  sizeBytes: number;
}

export interface Message {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  /** Data-URL images attached to a user turn — shown in the bubble and kept in
   *  the conversation so follow-up questions can still reference them. */
  images?: string[];
  model: string; // AI id (e.g. 'veebo', 'local-123') or 'user' for user messages
  thinking?: string;
  isLoading?: boolean;
  error?: string | null;
  aiLabel?: string;
  aiImageUrl?: string | null;
  statusText?: string; // Custom status text like "Model loading, please wait..."
  turnMode?: TurnMode; // the mode this message was generated under — drives thinking-window presentation + live-thinking gate
  turnModeAuto?: boolean; // true when turnMode was chosen by the classifier (not a sparkle/default) — for the auto badge
  /** True when this turn routed to a web-search model: it researches for a
   *  long stretch (often 30-60s) before any text, so the status line says
   *  "Searching the web" instead of a generic "thinking". */
  searchingWeb?: boolean;
  // OwnServer-only properties (unused in Desktop, but needed for TypeScript compatibility)
  tokens?: { prompt_tokens?: number; completion_tokens?: number; total_tokens?: number; tokens_per_second?: number };
  sources?: any[];
  /** Source-grounding: an answer's claims anchored to document quotes/spans (+
   *  image links), computed after the reply and shown in this message's Sources
   *  panel. Same shape recorded in the transcript provenance. */
  grounded?: {
    kind: "document" | "image";
    doc_sha256: string;
    doc_name?: string;
    claim?: string;
    quote?: string;
    span?: [number, number] | null;
  }[];
  /** True while the background source-grounding pass runs (a full local
   *  inference — can take many seconds), so the UI can show "verifying sources…"
   *  instead of an unexplained gap before the Sources button appears. */
  groundingPending?: boolean;
  /** Context stashed when auto-grounding is OFF, so the on-demand "Verify
   *  sources" button can run the pass for this answer. In-memory (this session)
   *  only — not recorded. */
  groundingSource?: { documentText: string; docSha256: string; docName?: string };
  /** This message's transcript action-hash hex (once recorded), so on-demand
   *  grounding can be persisted as an annotation linked to it. */
  transcriptHash?: string;
  /** Routing receipt: the model that actually served this assistant turn
   *  (gguf filename, "online:<id>", or "external:<id>"). Drives the hover
   *  strip under the reply. */
  servedBy?: string;
  /** Why the router picked servedBy (Auto modes only; undefined = the user
   *  picked the model). */
  routingReason?: string;
  /** The classified routing task at send time — reused when the user retries
   *  this turn online or on device. */
  routingTask?: string;
  showUpgradeButton?: boolean;
  originalUserQuery?: string;
  originalUserMessageContent?: string;
  /** Assistant bubble on the folder-agent path. Skips ChatMessage's
   *  last-reply min-height reservation - the agent turn reserves scroll
   *  space with one turn-scoped spacer instead, so interleaved activity
   *  cards don't grab/release space and bounce the view. */
  agentTurn?: boolean;
  /** Mid-turn continuation bubble (text after tool activity). Skips the
   *  question-to-top anchor: only the turn's FIRST bubble anchors, later
   *  segments print in place. */
  agentSegment?: boolean;
  /** A contiguous run of agent tool actions (folder open). A message with
   *  this set renders as a quiet collapsible activity card, not a bubble. */
  agentRun?: AgentRun;
  /** An agent permission request (folder open). Renders as an inline card;
   *  once answered it collapses to a one-line receipt. */
  agentPermission?: AgentPermission;
}

/** One tool action inside an agent activity run. */
export interface AgentAction {
  toolCallId: string;
  title: string;
  /** ACP tool kind: read | edit | delete | move | search | execute | fetch | ... */
  kind?: string;
  status: 'pending' | 'in_progress' | 'completed' | 'failed';
  /** File paths this action touched (feeds the changed-files viewer). */
  locations?: string[];
}

export interface AgentRun {
  actions: AgentAction[];
  /** Set when the turn ends; flips the card to its collapsed summary. */
  done?: boolean;
}

/** The exact thing the agent asked to do, straight from the ACP toolCall -
 *  rendered verbatim on the card, never paraphrased. */
export interface AgentPermission {
  requestId: number;
  title: string;
  /** ACP tool kind (drives the verb header + icon). */
  kind?: string;
  /** The exact command for execute asks - shown whole, never truncated. */
  command?: string;
  /** Unified-diff text for edit asks (collapsed preview + "show all"). */
  diff?: string;
  locations?: string[];
  options: { optionId: string; name: string; kind?: string }[];
  state: 'pending' | 'answered' | 'expired';
  /** One-line receipt after answering, e.g. "Allowed: npm install - once". */
  receipt?: string;
}

export type ChatAction = 'Write a report...' | 'Write code...' | null;

export interface AttachedFile {
  filename: string;
  path: string;
  sizeBytes: number;
  content: string;
  truncated: boolean;
  estimatedTokens: number;
}

export interface SelectedAiModel {
  id: string;
  label: string;
  imageUrl?: string | null;
  aiConfig: UserDefinedAI;
}

// Holochain transcript types
export interface HolochainConversation {
  hash: string;
  ai_personality_id: string;
  ai_personality_name: string;
  model_used: string;
  started_at: number;
  title: string | null;
  /** External app that drove this conversation over the API (null = in-app). */
  source?: string | null;
  /** Agent (generation) whose chain holds this conversation — pass this
   *  to getTranscript, not the AI's current agent key. */
  agent_key: string;
}

export interface HolochainTokenUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  tokens_per_second: number | null;
}

export interface HolochainTranscriptEntry {
  hash: string;
  role: string;
  content: string;
  sequence: number;
  timestamp: number;
  model: string;
  thinking: string | null;
  tokens: HolochainTokenUsage | null;
  // Provenance (recorded from Phase A+; null on older entries)
  sources?: { url: string; title: string }[] | null;
  system_prompt?: string | null;
  mode?: string | null;
  attachments?: { bytes: number; sha256: string; content?: string | null } | null;
  images?: {
    filename: string;
    mime: string;
    bytes: number;
    sha256: string;
    content?: string | null;
  }[] | null;
  grounded?: {
    kind: "document" | "image";
    doc_sha256: string;
    doc_name?: string | null;
    claim?: string | null;
    quote?: string | null;
    span?: [number, number] | null;
  }[] | null;
  runtime?: { app_version: string; online: boolean; max_tokens?: number | null } | null;
  routing_reason?: string | null;
  routing_task?: string | null;
}
