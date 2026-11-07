#!/usr/bin/env node
// The four sidecars' SHA-256 as they ship: read from inside the final package
// (after code signing), one record per sidecar. A release publishes them
// beside its downloads; the next release compiles them into its kit
// descriptors (`KitBinary::accepted`), so the binaries the kit cache restores
// are checked against hashes from a signed release, not from a build log.
//
// Usage: node scripts/kit-hashes.mjs <target triple> <bundle dir> <out.json>
//   macOS: the .app's Contents/MacOS; Linux: the .deb; Windows: the NSIS
//   installer (unpacked with 7-Zip).
import { createHash } from 'node:crypto';
import { existsSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';

const [target, bundle, out] = process.argv.slice(2);
if (!target || !bundle || !out) {
  console.error('usage: node scripts/kit-hashes.mjs <target triple> <bundle dir> <out.json>');
  process.exit(2);
}
const SIDECAR = /^yourowai-(holochain|lair-keystore)(-07)?(\.exe)?$/;
const base = (f) => f.split(/[\\/]/).pop();
const walk = (dir) => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
const one = (dir, re) => {
  const found = existsSync(dir) ? readdirSync(dir).filter((n) => re.test(n)) : [];
  if (found.length !== 1) throw new Error(`expected one ${re} in ${dir}, found ${found.length}`);
  return join(dir, found[0]);
};

let root;
if (target.includes('apple-darwin')) {
  root = join(one(join(bundle, 'macos'), /\.app$/), 'Contents', 'MacOS');
} else if (target.includes('linux')) {
  root = mkdtempSync(join(tmpdir(), 'kit-'));
  execFileSync('dpkg-deb', ['-x', one(join(bundle, 'deb'), /\.deb$/), root]);
} else if (target.includes('windows')) {
  root = mkdtempSync(join(tmpdir(), 'kit-'));
  execFileSync('7z', ['x', '-y', `-o${root}`, one(join(bundle, 'nsis'), /-setup\.exe$/)], { stdio: 'ignore' });
} else {
  throw new Error(`unknown target ${target}`);
}

const files = walk(root).filter((f) => SIDECAR.test(base(f)));
const records = files
  .map((f) => {
    const sidecar = base(f).replace(/\.exe$/, '');
    // The -07 sidecars are the 0.7 line's; the others the 0.6 line's.
    return {
      line: sidecar.endsWith('-07') ? 'holochain-0.7' : 'holochain-0.6',
      target,
      sidecar,
      sha256: createHash('sha256').update(readFileSync(f)).digest('hex'),
    };
  })
  .sort((a, b) => a.sidecar.localeCompare(b.sidecar));
if (records.length !== 4 || new Set(records.map((r) => r.sidecar)).size !== 4) {
  throw new Error(`expected the four sidecars once each, found: ${files.map(base).join(', ') || 'none'}`);
}
writeFileSync(out, JSON.stringify(records, null, 2) + '\n');
for (const r of records) console.log(`${r.sha256}  ${r.target}  ${r.sidecar}`);
