#!/usr/bin/env node
// When memory's search part is fetched for a person with no model on this
// computer. It is fetched by itself only when one of their AIs answers
// online and the connection is not metered; on a metered connection the
// person is asked first; with no online AI nothing is fetched. This check
// holds that decision (src/utils/partsStep.ts) to the table below.
import { onlineOnlyStep } from '../src/utils/partsStep.ts';

const CASES = [
  [[], false, 'no-ais-yet'],
  [[], true, 'no-ais-yet'],
  [['Qwen3.8-4B-Q4_K_M.gguf'], false, 'no-ais-yet'],
  [['auto', 'auto:offline'], false, 'no-ais-yet'],
  [['online:some-model'], false, 'fetch'],
  [['online:some-model'], true, 'ask-first'],
  [['auto:online'], false, 'fetch'],
  [['auto:online'], true, 'ask-first'],
  [['auto:online-offline'], false, 'fetch'],
  [['auto:online-offline'], true, 'ask-first'],
  [['Qwen3.8-4B-Q4_K_M.gguf', 'online:some-model'], true, 'ask-first'],
  [['', 'online:some-model'], false, 'fetch'],
  // A name that only contains the word is not an online AI.
  [['my-online:model.gguf'], false, 'no-ais-yet'],
];

const problems = [];
for (const [aiModels, metered, want] of CASES) {
  const got = onlineOnlyStep({ aiModels, metered });
  if (got !== want) problems.push(`${JSON.stringify(aiModels)}${metered ? ' on a metered connection' : ''}: ${got}, expected ${want}`);
}
if (problems.length) {
  console.error('check-parts-step: the decision changed:\n  ' + problems.join('\n  '));
  process.exit(1);
}
console.log(`check-parts-step: ${CASES.length} cases hold`);
