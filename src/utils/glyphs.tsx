import { component$ } from "@builder.io/qwik";
import {
  LuActivity, LuBanknote, LuBarChart, LuBook, LuBookOpen, LuBookmark, LuBox, LuBrain, LuBriefcase, LuBug,
  LuCalculator, LuCalendar, LuCamera, LuClipboard, LuCloud, LuCode, LuCompass, LuCpu, LuDatabase, LuDna,
  LuFileText, LuFlame, LuFlaskConical, LuFolder, LuGamepad2, LuGavel, LuGitBranch, LuGlobe, LuGraduationCap,
  LuHammer, LuHeart, LuHome, LuImage, LuKey, LuLanguages, LuLayout, LuLeaf, LuLightbulb, LuList, LuLock,
  LuMail, LuMap, LuMessageSquare, LuMic, LuMusic, LuNewspaper, LuPackage, LuPalette, LuPenTool, LuPencil,
  LuPlane, LuPrinter, LuPuzzle, LuRocket, LuScale, LuScissors, LuSearch, LuServer, LuSettings, LuShield,
  LuShoppingCart, LuSparkles, LuStar, LuStethoscope, LuTable, LuTag, LuTerminal, LuTestTube, LuTruck,
  LuUsers, LuVideo, LuWrench, LuZap,
} from "@qwikest/icons/lucide";

/**
 * The glyphs a skill may name in its SKILL.md front matter (`icon:
 * book-open`), by their Lucide names. A curated set, not the whole
 * library: every glyph here ships in the bundle. Unknown names draw
 * nothing, and the caller falls back (a skill: the puzzle piece).
 */
export const GLYPH_NAMES = [
  "activity", "banknote", "bar-chart", "book", "book-open", "bookmark", "box", "brain", "briefcase", "bug",
  "calculator", "calendar", "camera", "clipboard", "cloud", "code", "compass", "cpu", "database", "dna",
  "file-text", "flame", "flask", "folder", "gamepad", "gavel", "git-branch", "globe", "graduation-cap",
  "hammer", "heart", "home", "image", "key", "languages", "layout", "leaf", "lightbulb", "list", "lock",
  "mail", "map", "message-square", "mic", "music", "newspaper", "package", "palette", "pen-tool", "pencil",
  "plane", "printer", "puzzle", "rocket", "scale", "scissors", "search", "server", "settings", "shield",
  "shopping-cart", "sparkles", "star", "stethoscope", "table", "tag", "terminal", "test-tube", "truck",
  "users", "video", "wrench", "zap",
] as const;

export function knownGlyph(name: string | null | undefined): boolean {
  return !!name && (GLYPH_NAMES as readonly string[]).includes(name);
}

/** One named glyph; nothing for a name the set does not carry. */
export const Glyph = component$<{ name: string; class?: string }>((props) => {
  const c = props.class ?? "h-3.5 w-3.5";
  switch (props.name) {
    case "activity": return <LuActivity class={c} />;
    case "banknote": return <LuBanknote class={c} />;
    case "bar-chart": return <LuBarChart class={c} />;
    case "book": return <LuBook class={c} />;
    case "book-open": return <LuBookOpen class={c} />;
    case "bookmark": return <LuBookmark class={c} />;
    case "box": return <LuBox class={c} />;
    case "brain": return <LuBrain class={c} />;
    case "briefcase": return <LuBriefcase class={c} />;
    case "bug": return <LuBug class={c} />;
    case "calculator": return <LuCalculator class={c} />;
    case "calendar": return <LuCalendar class={c} />;
    case "camera": return <LuCamera class={c} />;
    case "clipboard": return <LuClipboard class={c} />;
    case "cloud": return <LuCloud class={c} />;
    case "code": return <LuCode class={c} />;
    case "compass": return <LuCompass class={c} />;
    case "cpu": return <LuCpu class={c} />;
    case "database": return <LuDatabase class={c} />;
    case "dna": return <LuDna class={c} />;
    case "file-text": return <LuFileText class={c} />;
    case "flame": return <LuFlame class={c} />;
    case "flask": return <LuFlaskConical class={c} />;
    case "folder": return <LuFolder class={c} />;
    case "gamepad": return <LuGamepad2 class={c} />;
    case "gavel": return <LuGavel class={c} />;
    case "git-branch": return <LuGitBranch class={c} />;
    case "globe": return <LuGlobe class={c} />;
    case "graduation-cap": return <LuGraduationCap class={c} />;
    case "hammer": return <LuHammer class={c} />;
    case "heart": return <LuHeart class={c} />;
    case "home": return <LuHome class={c} />;
    case "image": return <LuImage class={c} />;
    case "key": return <LuKey class={c} />;
    case "languages": return <LuLanguages class={c} />;
    case "layout": return <LuLayout class={c} />;
    case "leaf": return <LuLeaf class={c} />;
    case "lightbulb": return <LuLightbulb class={c} />;
    case "list": return <LuList class={c} />;
    case "lock": return <LuLock class={c} />;
    case "mail": return <LuMail class={c} />;
    case "map": return <LuMap class={c} />;
    case "message-square": return <LuMessageSquare class={c} />;
    case "mic": return <LuMic class={c} />;
    case "music": return <LuMusic class={c} />;
    case "newspaper": return <LuNewspaper class={c} />;
    case "package": return <LuPackage class={c} />;
    case "palette": return <LuPalette class={c} />;
    case "pen-tool": return <LuPenTool class={c} />;
    case "pencil": return <LuPencil class={c} />;
    case "plane": return <LuPlane class={c} />;
    case "printer": return <LuPrinter class={c} />;
    case "puzzle": return <LuPuzzle class={c} />;
    case "rocket": return <LuRocket class={c} />;
    case "scale": return <LuScale class={c} />;
    case "scissors": return <LuScissors class={c} />;
    case "search": return <LuSearch class={c} />;
    case "server": return <LuServer class={c} />;
    case "settings": return <LuSettings class={c} />;
    case "shield": return <LuShield class={c} />;
    case "shopping-cart": return <LuShoppingCart class={c} />;
    case "sparkles": return <LuSparkles class={c} />;
    case "star": return <LuStar class={c} />;
    case "stethoscope": return <LuStethoscope class={c} />;
    case "table": return <LuTable class={c} />;
    case "tag": return <LuTag class={c} />;
    case "terminal": return <LuTerminal class={c} />;
    case "test-tube": return <LuTestTube class={c} />;
    case "truck": return <LuTruck class={c} />;
    case "users": return <LuUsers class={c} />;
    case "video": return <LuVideo class={c} />;
    case "wrench": return <LuWrench class={c} />;
    case "zap": return <LuZap class={c} />;
    default: return null;
  }
});
