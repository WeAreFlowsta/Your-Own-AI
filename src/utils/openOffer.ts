/**
 * An open offer: one thing the app offers after a reply, as a short
 * button above the message box, for the newest reply only. Pressing it
 * sends its words as the person's message. While it is open, a short
 * answer in the person's own words is read against it first: with one
 * thing on offer, "yes" or "go on" can only mean that, so many ways of
 * saying it are safe. Anything else is an ordinary message, and the offer
 * is gone. An offer has its words to show (and, with voice, to say) and
 * the short answers that take it.
 *
 * No imports: the build check (scripts/check-open-offer.mjs) runs this.
 */
export interface OpenOffer {
  /** Which offer it is. */
  id: string;
  /** On the button, and sent as the person's message when pressed. */
  say: string;
  /** Short answers that take the offer, besides its own words. Lower
   *  case, no punctuation. */
  takes: string[];
}

/** Answers that take whatever is on offer. */
const YES = [
  "yes", "yes please", "yes do", "yes do that", "yes go on", "yes go ahead", "yep", "yeah", "yeah go on", "sure", "ok", "okay", "ok do that", "ok go on",
  "please", "please do", "do it", "do that", "go on", "go ahead", "go for it",
];

/** After a reply that was given only the closest of what memory found. */
export const LOOK_FURTHER: OpenOffer = {
  id: "look-further",
  say: "Look further",
  takes: [
    "look further", "look farther", "look deeper", "look again", "look harder", "look more", "look some more", "keep looking", "keep searching", "keep going",
    "search further", "search more", "search deeper", "search again", "search harder", "dig deeper", "dig further", "check further", "check again", "try again",
    "look further back", "look back further", "search everything", "look at everything", "look through everything", "look at the rest", "check the rest", "search the rest",
  ],
};

/** A message as it is compared: lower case, marks and doubled spaces
 *  gone, and the small words people wrap a request in taken off. */
function plain(message: string): string {
  let said = message.toLowerCase().replace(/[’']/g, "").replace(/[^a-z0-9 ]+/g, " ").replace(/\s+/g, " ").trim();
  for (;;) {
    const before = said;
    said = said.replace(/^(please|can you|could you|would you|will you|then|and|so|well|hmm|um) /, "");
    said = said.replace(/ (please|then|for me|now|in memory|in my memory|in your memory|through memory|through my conversations|in my conversations)$/, "");
    if (said === before) break;
  }
  return said;
}

/** Does this message take the offer? Only a short one that says so and
 *  nothing else; a question never does. */
export function takesOffer(offer: OpenOffer, message: string): boolean {
  if (message.length > 60 || message.trim().endsWith("?")) return false;
  const said = plain(message);
  if (!said) return false;
  if (said === plain(offer.say) || offer.takes.includes(said) || YES.includes(said)) return true;
  // "Yes, look further": a yes and then the offer's own words.
  for (const yes of ["yes", "yep", "yeah", "ok", "okay", "sure"]) {
    if (said.startsWith(yes + " ")) {
      const rest = plain(said.slice(yes.length + 1));
      if (rest === plain(offer.say) || offer.takes.includes(rest) || YES.includes(rest)) return true;
    }
  }
  return false;
}
