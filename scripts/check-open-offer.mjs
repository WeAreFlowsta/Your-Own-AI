#!/usr/bin/env node
// An open offer is taken by its own words, by a short yes, and by the
// ways people say the same thing; never by a question or by an ordinary
// message. This check holds that reading (src/utils/openOffer.ts) to the
// two lists below.
import { LOOK_FURTHER, takesOffer } from '../src/utils/openOffer.ts';

const TAKES = [
  'Look further', 'look further.', 'Look further please', 'Yes', 'yes please', 'Yes, look further', 'ok', 'Okay, go on', 'Go on', 'go ahead',
  'Keep looking', 'please keep looking', 'Search more', 'can you look deeper', 'Could you search further, please', 'dig deeper', 'look again',
  'Look further in my conversations', 'yeah go on', 'Sure', 'do it', 'check the rest', 'Look harder!', 'So look further then',
];
const ORDINARY = [
  '', 'No', 'no thanks', 'Not now', 'Should I look further?', 'Look further into why the oven is on the left', 'What is the free delivery threshold?',
  'Look', 'Further', 'yes but first tell me about the flour', 'ok so what about the delivery days', 'Go on holiday', 'Keep looking at the budget for me and tell me when it changes',
  'Look further than the price when you choose a supplier', 'Yes?', 'ok thanks', 'thanks', 'search', 'Try the other supplier again',
];

const problems = [];
for (const m of TAKES) if (!takesOffer(LOOK_FURTHER, m)) problems.push(`"${m}" should take the offer`);
for (const m of ORDINARY) if (takesOffer(LOOK_FURTHER, m)) problems.push(`"${m}" is an ordinary message`);
if (problems.length) {
  console.error('check-open-offer: the reading changed:\n  ' + problems.join('\n  '));
  process.exit(1);
}
console.log(`check-open-offer: ${TAKES.length} that take it and ${ORDINARY.length} ordinary messages hold`);
