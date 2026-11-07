// Fails unless a release tag matches the version in every place that carries one:
// `tauri.conf.json` (names the installers), the Cargo workspace and the web package.
//
//   node scripts/ci/check-version.mjs v0.2.0

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const read = (path) => readFileSync(join(root, path), 'utf8');

const tag = process.argv[2];
if (!tag || !/^v\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(tag)) {
  console.error(`Expected a tag like v0.2.0, got "${tag ?? ''}".`);
  process.exit(1);
}
const expected = tag.slice(1);

const workspace = read('Cargo.toml').match(/\[workspace\.package\][^[]*?\bversion\s*=\s*"([^"]+)"/)?.[1];
const found = {
  'apps/desktop/src-tauri/tauri.conf.json': JSON.parse(read('apps/desktop/src-tauri/tauri.conf.json')).version,
  'Cargo.toml [workspace.package]': workspace,
  'apps/desktop/web/package.json': JSON.parse(read('apps/desktop/web/package.json')).version,
};

let bad = false;
for (const [where, version] of Object.entries(found)) {
  const ok = version === expected;
  console.log(`${ok ? 'ok ' : 'BAD'} ${where}: ${version}`);
  bad ||= !ok;
}
if (bad) {
  console.error(`\nTag ${tag} does not match the versions above. Bump them (and commit) before tagging.`);
  process.exit(1);
}
