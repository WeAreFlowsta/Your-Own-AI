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
  /** Set when the file does not read as a model (incomplete or corrupted
   *  download): the reason. Listed so it can be deleted, never offered. */
  damaged?: string;
  /** A sharded model: how many files make it up (`name` is the first). */
  shard_count?: number;
  /** The registered speed-up (speculative-decoding) file beside this model. */
  draft?: string;
  /** sha256 of the file once the app has hashed it; compared with the catalog's pin. */
  sha256?: string;
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
  /** Names of documents attached to a user turn (PDF, spreadsheet, text).
   *  Their extracted text goes to the model as context, NEVER into the
   *  bubble - the bubble shows a file chip per name instead. */
  attachedFiles?: string[];
  model: string; // AI id (e.g. 'veebo', 'local-123') or 'user' for user messages
  thinking?: string;
  isLoading?: boolean;
  /** The user stopped this reply - shown and recorded as far as it got. */
  stopped?: boolean;
  /** Why Verify sources could not run, or that it matched nothing. */
  groundingNote?: string;
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
  tokens?: { prompt_tokens?: number; completion_tokens?: number; total_tokens?: number; tokens_per_second?: number; prompt_per_second?: number; engine?: string };
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
  /** The reply bubble of a folder-agent turn (ONE per turn). Skips
   *  ChatMessage's per-bubble min-height reservation - the agent turn
   *  reserves scroll space with one turn-scoped spacer instead. */
  agentTurn?: boolean;
  /** The agent turn's working log, rendered as the work rail inside the
   *  reply bubble (steps + narration + thoughts + permissions, in true
   *  order). `content` holds only the text the AI is currently saying -
   *  which, at turn end, IS the final answer. */
  agentLog?: AgentLogItem[];
  /** Turn-level stats from the agent's turn_completed usage - drives the
   *  collapsed stub ("6 steps - 5 files - 40s") and the Tokens panel. */
  agentStats?: { durationMs?: number; modelCalls?: number };
  /** Every permission decision of the turn, compactly (see PermissionLedger). */
  permissionLedger?: PermissionLedger;
  /** An agent permission request (folder open). Renders as an inline card;
   *  once answered it collapses to a one-line receipt. */
  agentPermission?: AgentPermission;
}

/** One entry in an agent turn's working log.
 *  `id` is assigned ONCE at creation and preserved through every update -
 *  it is the render key. Index keys made Qwik re-match elements across
 *  item types during the turn's rapid inserts, mis-nesting rows into each
 *  other's flex buttons (labels wrapping word-by-word, text over text). */
export type AgentLogItem = { id: string } & (
  | { type: 'action'; action: AgentAction }
  /** Text the AI said mid-work, superseded by later activity - shown muted
   *  inside the box, without bubble chrome. */
  | { type: 'narration'; text: string }
  /** Model reasoning (agent_thought_chunk). Shown only when the user turns
   *  on the thinking view. */
  | { type: 'thought'; text: string }
  /** A permission ask at its true position in the work. Pending = the full
   *  card; answered = its receipt line. */
  | { type: 'permission'; permission: AgentPermission }
  /** The agent's live task plan (ACP plan updates). Each update replaces
   *  the entries wholesale; the item keeps its place in the log. */
  | { type: 'plan'; entries: AgentPlanEntry[] }
);

/** One task in the agent's plan checklist. */
export interface AgentPlanEntry {
  content: string;
  priority?: 'high' | 'medium' | 'low';
  status: 'pending' | 'in_progress' | 'completed';
}

/** One tool action inside an agent turn's working log. */
export interface AgentAction {
  toolCallId: string;
  /** Humanized: "Reading package.json", "Running npm test". */
  label: string;
  /** ACP/x.ai tool kind: read | edit | list | search | execute | fetch... */
  kind?: string;
  status: 'pending' | 'in_progress' | 'completed' | 'failed';
  /** File paths this action touched (feeds the changed-files viewer). */
  locations?: string[];
  /** The full input (path, command...) for the expanded view. */
  detail?: string;
  /** Result preview (directory tree, file text, command output). */
  output?: string;
  /** Total line count of the result, for the "· 84 lines" hint. */
  outputLines?: number;
  /** Edit result as a real diff (ACP diff content). `lines` render the
   *  colored view live; only the counts survive into the transcript. */
  diff?: AgentActionDiff;
  /** The latest line of a background task's live log (tailed from the
   *  agent's terminal logs while the turn runs). Display-only - never
   *  persisted. */
  liveLine?: string;
  /** For a wait step: the tool-call ids of the backgrounded tasks it is
   *  blocked on - the terminal logs the tailer should read for it. */
  waitFor?: string[];
}

export interface AgentActionDiff {
  path: string;
  added: number;
  removed: number;
  lines?: import('../utils/lineDiff').DiffLine[];
}

/** The exact thing the agent asked to do, straight from the ACP toolCall -
 *  rendered verbatim on the card, never paraphrased. */
export interface AgentPermission {
  requestId: number;
  /** The tool call this ask belongs to - on Allow, the rail grows the
   *  step immediately (the agent stays silent while executing). */
  toolCallId?: string;
  title: string;
  /** ACP tool kind (drives the verb header + icon). */
  kind?: string;
  /** The exact command for execute asks - shown whole, never truncated. */
  command?: string;
  /** The exact payload for tool asks with no command - e.g. the NOTE the
   *  agent wants to remember. Shown verbatim on the card. */
  detail?: string;
  /** The edit shown as a real diff (collapsed preview + "show all"). */
  diff?: AgentActionDiff;
  locations?: string[];
  options: { optionId: string; name: string; kind?: string }[];
  state: 'pending' | 'answered' | 'expired';
  /** One-line receipt after answering, e.g. "Allowed: npm install - once". */
  receipt?: string;
  /** Structured outcome, recorded with the turn so a grant can be audited
   *  as data, not parsed out of the receipt line. */
  decision?: 'allow' | 'reject';
  /** once = this ask only; always = the agent persists the grant for this
   *  folder (its own store, outside the record). */
  scope?: 'once' | 'always';
  /** The ACP option kind that was selected (allow_once, allow_always, ..). */
  optionKind?: string;
  /** ISO time the ask was answered or expired. */
  answeredAt?: string;
  /** How it was answered: a card button, a typed reply (= decline once),
   *  the app's own policy (auto), or never (expired with the turn). */
  via?: 'button' | 'reply' | 'auto' | 'expired';
  /** For via "auto": which judge allowed it - the harness's routine
   *  fast path, its rule-based classifier, a model, or the app's own policy. */
  autoReason?: 'fast_path' | 'heuristic' | 'model' | 'app_policy' | 'always';
  /** Why an ask reached the user while Auto was on (from the harness's
   *  prompt trigger on the request) - shown on the card. */
  promptReason?: string;
}

/** Compact per-turn ledger of EVERY permission decision the harness made -
 *  including the ones no card ever showed (reads allowed by policy, grants
 *  the user made earlier). Recorded with the turn; not rendered row by row. */
export interface PermissionLedger {
  /** Decisions by reason, e.g. { static_allowlist: 40, auto_fast_path: 12 }. */
  byReason: Record<string, number>;
  /** Total decisions this turn. */
  total: number;
  /** How many were auto-approved (fast path or classifier) vs prompted. */
  autoApproved: number;
  prompted: number;
  /** The permission mode the harness reported for the turn. */
  mode?: string;
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
  /** When a turn was last recorded (micros); absent for conversations not
   *  continued since this existed - order by it, falling back to started_at. */
  last_active_at?: number | null;
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
  /** Reply stopped by the user - recorded as far as it got. */
  stopped?: boolean | null;
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
  /** Agent turn: the working log ({ items, stats }) - restores the rail on
   *  resume. Client-side schema; old entries read back null. */
  agent_log?: { items?: AgentLogItem[]; stats?: Message['agentStats'] } | null;
  /** Workspace folder the turn worked in. */
  folder_path?: string | null;
}
