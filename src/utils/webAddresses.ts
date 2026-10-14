/**
 * Web addresses in a message: full links, and bare domains the way people
 * type them ("what is on flowsta.com", "www.bbc.co.uk/news"). File names
 * ("notes.md", "model.gguf") and abbreviations ("e.g.") are not addresses.
 * Each result is a full https address; the Rust side applies the address
 * rules (local names and addresses are never read).
 */
const TLDS = new Set([
  "com", "net", "org", "io", "ai", "co", "dev", "app", "info", "edu", "gov", "biz", "me", "tv", "xyz", "site",
  "online", "tech", "shop", "store", "news", "blog", "page", "cloud", "wiki", "uk", "au", "nz", "de", "fr", "nl",
  "ca", "us", "eu", "ie", "es", "it", "se", "no", "dk", "fi", "ch", "at", "be", "pt", "pl", "cz", "jp", "kr", "in",
  "br", "mx", "za", "sg", "hk",
]);

const FULL = /https?:\/\/[^\s<>()"'\]]+/gi;
const BARE = /(?<![\w@/.-])((?:[a-z0-9-]+\.)+[a-z]{2,})((?:\/[^\s<>()"'\]]*)?)/gi;

export function findWebAddresses(text: string, max = 2): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  const push = (raw: string) => {
    const url = raw.replace(/[.,;:!?)]+$/, "");
    const key = url.toLowerCase().replace(/\/$/, "");
    if (!seen.has(key)) {
      seen.add(key);
      out.push(url);
    }
  };
  for (const m of text.matchAll(FULL)) push(m[0]);
  const withoutFull = text.replace(FULL, " ");
  for (const m of withoutFull.matchAll(BARE)) {
    const host = m[1].toLowerCase();
    const labels = host.split(".");
    const tld = labels[labels.length - 1];
    if (!TLDS.has(tld)) continue;
    // "co.uk" and friends: the second label is a known suffix too, fine; a
    // single-label host is never an address.
    if (labels.length < 2) continue;
    push(`https://${m[1]}${m[2] ?? ""}`);
  }
  return out.slice(0, max);
}
