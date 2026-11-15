// Who is recommended what. Every recommendation in the app comes from one
// order (rankPicks in src/data/recommended-models.ts): where a model runs,
// then the app's own grade for the job. This check runs that order over the
// real catalog and the real grades (scripts/catalog-grades.json, written by a
// Rust test) for a set of example computers, and holds the first model each
// one is given to the table below. A catalog or grade change that moves a
// first pick fails the build until the table is changed on purpose.
//
// The fit here is a plain stand-in for the app's grader (which reads the real
// machine): on the card when the file plus 1.5 GB fits its memory, a split
// model with a card, else the processor when minRAM fits.
// `node scripts/check-model-picks.mjs --print` shows every pick.
import { readFileSync } from 'node:fs';
import { modelFamilies, rankPicks, picksFor, isMoeVariant, getModality } from '../src/data/recommended-models.ts';

const grades = JSON.parse(readFileSync(new URL('./catalog-grades.json', import.meta.url), 'utf8'));
const MACHINES = [
  { name: '8 GB, no card', ram: 8, vram: 0 },
  { name: '16 GB, no card', ram: 16, vram: 0 },
  { name: '16 GB, 4 GB card', ram: 16, vram: 4 },
  { name: '16 GB, 8 GB card', ram: 16, vram: 8 },
  { name: '32 GB, 16 GB card', ram: 32, vram: 16 },
  { name: '64 GB, 24 GB card', ram: 64, vram: 24 },
];
// The first model a new person is given, per example computer.
const FIRST = {
  '8 GB, no card': 'gemma-4',
  '16 GB, no card': 'gemma-4',
  '16 GB, 4 GB card': 'phi-4-mini',
  '16 GB, 8 GB card': 'phi-4-mini',
  '32 GB, 16 GB card': 'qwen-3.6',
  '64 GB, 24 GB card': 'qwen-3.8',
};

function modesFor(m) {
  const modes = {};
  for (const f of modelFamilies) for (const v of f.variants) {
    const mode = m.vram > 0 && v.size + 1.5 <= m.vram ? 'gpu'
      : m.vram > 0 && isMoeVariant(v) && v.minRAM <= m.ram ? 'moe-split'
      : v.minRAM <= m.ram ? 'cpu' : 'too-big';
    modes[v.filename] = { mode, fit: 'green', grades: grades[v.filename] };
  }
  return modes;
}
const general = (p) => !p.variant.filename.toLowerCase().includes('medgemma');
const best = (match, modes, grade, opts) => rankPicks(picksFor(modelFamilies.filter(match), modes).filter(general), modes, grade, opts)[0];
const show = (p, modes, grade) => p ? `${p.family.id} ${p.variant.parameterCount} (${p.variant.size} GB, ${modes[p.variant.filename].mode}, grade ${modes[p.variant.filename].grades?.[grade]})` : 'none';

const print = process.argv.includes('--print');
const wrong = [];
for (const f of modelFamilies) for (const v of f.variants) if (!grades[v.filename]) wrong.push(`${v.filename} has no grade: run UPDATE_CATALOG_GRADES=1 cargo test --lib catalog_grades_file`);
for (const m of MACHINES) {
  const modes = modesFor(m);
  const first = best((f) => f.recommended, modes, 'overall', { firstLaunch: true });
  if (print) {
    console.log(`\n${m.name}`);
    console.log(`  first model:   ${show(first, modes, 'overall')}`);
    console.log(`  everyday chat: ${show(best((f) => f.capabilities.includes('chat'), modes, 'overall'), modes, 'overall')}`);
    console.log(`  project work:  ${show(best((f) => f.capabilities.includes('agentic'), modes, 'agent'), modes, 'agent')}`);
    console.log(`  images:        ${show(best((f) => getModality(f).in.includes('vision'), modes, 'vision'), modes, 'vision')}`);
  }
  if (first?.family.id !== FIRST[m.name]) wrong.push(`${m.name}: the first model is ${first?.family.id ?? 'none'}, the table says ${FIRST[m.name]}`);
}
if (wrong.length) { console.error('check-model-picks:\n  ' + wrong.join('\n  ')); process.exit(1); }
if (!print) console.log(`check-model-picks: ${MACHINES.length} example computers get the first model the table names`);
