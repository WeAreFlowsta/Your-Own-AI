/**
 * Recommended skills - the shelf on day one, before the site directory
 * exists. Every entry is an open-standard skill folder in a public repo,
 * pinned to the commit it was reviewed at (the link carries the commit, so
 * "Add" installs exactly that version; the card's update check tells the
 * user when the source has moved on).
 *
 * Wave-1 rules (planning/SKILLS.md): permissive license, knowledge only
 * (no scripts / hooks / MCP), SKILL.md small enough for a local model's
 * context, useful beyond the terminal, maintained. Reviewed 2026-08-27.
 */

export type SkillGroup = "Build" | "Writing" | "Work";

export interface RecommendedSkill {
  /** Folder name the install produces (front matter `name`). */
  name: string;
  title: string;
  group: SkillGroup;
  /** Our one-liner - what it does for the person, not the repo's blurb. */
  blurb: string;
  maker: string;
  license: string;
  /** Pinned install link: github.com/<owner>/<repo>/tree/<commit>[/<path>]. */
  link: string;
  /** SKILL.md size in characters at review time (about a quarter of that in tokens). */
  sizeChars: number;
  /** The listing's pinned commit (directory entries) - an installed copy on
   *  an older commit has an update waiting. */
  commit?: string;
  /** The maker's Flowsta username when the listing is claimed: the card
   *  reads "Signed by @username" instead of the name as listed. */
  signedBy?: string | null;
}

export const RECOMMENDED_SKILLS: RecommendedSkill[] = [
  {
    name: "holochain",
    title: "Holochain development",
    group: "Build",
    blurb: "Build Holochain apps the right way: zome architecture, entry and link design, validation, testing with Sweettest, packaging. Pinned to HDK 0.6.",
    maker: "Sacha Pignot (Soushi888)",
    license: "Apache-2.0",
    link: "https://github.com/Soushi888/holochain-agent-skills/tree/02791979533c48c9eca3fe261d972fec6c1b7bfb",
    sizeChars: 8814,
  },
  {
    name: "debugging-and-error-recovery",
    title: "Debugging and error recovery",
    group: "Build",
    blurb: "Find the root cause instead of guessing when a test fails, a build breaks, or behavior does not match expectations.",
    maker: "Addy Osmani",
    license: "MIT",
    link: "https://github.com/addyosmani/agent-skills/tree/5a5ea45e806f82273549fd85e60adb95d55f510d/skills/debugging-and-error-recovery",
    sizeChars: 10837,
  },
  {
    name: "incremental-implementation",
    title: "Incremental implementation",
    group: "Build",
    blurb: "Land changes in small, checkable steps when a task touches more than one file or feels too big for one go.",
    maker: "Addy Osmani",
    license: "MIT",
    link: "https://github.com/addyosmani/agent-skills/tree/5a5ea45e806f82273549fd85e60adb95d55f510d/skills/incremental-implementation",
    sizeChars: 9507,
  },
  {
    name: "documentation-and-adrs",
    title: "Documentation and decision records",
    group: "Build",
    blurb: "Write down the decisions and context the next person (or AI) will need: architecture decision records, API changes, shipped features.",
    maker: "Addy Osmani",
    license: "MIT",
    link: "https://github.com/addyosmani/agent-skills/tree/5a5ea45e806f82273549fd85e60adb95d55f510d/skills/documentation-and-adrs",
    sizeChars: 9782,
  },
  {
    name: "frontend-design",
    title: "Front-end design",
    group: "Build",
    blurb: "Distinctive, intentional visual design for new or reshaped UI - direction, typography, and choices that do not read as template defaults.",
    maker: "Anthropic",
    license: "Apache-2.0",
    link: "https://github.com/anthropics/skills/tree/3b3fad96af16a10759d930941b4520ba0c40edae/skills/frontend-design",
    sizeChars: 8260,
  },
  {
    name: "idea-refine",
    title: "Refine an idea",
    group: "Work",
    blurb: "Turn a vague idea into a sharp, testable one: widen the options, stress-test the assumptions, then converge on a plan.",
    maker: "Addy Osmani",
    license: "MIT",
    link: "https://github.com/addyosmani/agent-skills/tree/5a5ea45e806f82273549fd85e60adb95d55f510d/skills/idea-refine",
    sizeChars: 8111,
  },
  {
    name: "internal-comms",
    title: "Internal communications",
    group: "Writing",
    blurb: "Status reports, leadership updates, newsletters, FAQs, incident and project updates - in the formats teams actually use.",
    maker: "Anthropic",
    license: "Apache-2.0",
    link: "https://github.com/anthropics/skills/tree/3b3fad96af16a10759d930941b4520ba0c40edae/skills/internal-comms",
    sizeChars: 1511,
  },
  {
    name: "discernment-nudge",
    title: "Second look before you act",
    group: "Writing",
    blurb: "After advice, a draft, an estimate or an analysis you might act on, the AI adds a short note on what to double-check before relying on it.",
    maker: "Anthropic",
    license: "Apache-2.0",
    link: "https://github.com/anthropics/skills/tree/3b3fad96af16a10759d930941b4520ba0c40edae/skills/discernment-nudge",
    sizeChars: 10592,
  },
];

export const SKILL_GROUPS: SkillGroup[] = ["Build", "Writing", "Work"];
