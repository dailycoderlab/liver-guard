import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { buildManifest } from './generate-update-manifest.mjs';

const SCRIPT = fileURLToPath(new URL('./generate-update-manifest.mjs', import.meta.url));
const date = new Date('2026-11-11T09:00:00Z');

const signatures = {
  'LiverGuard.app.tar.gz': 'mac-signature\n',
  'LiverGuard_0.2.1_x64-setup.exe': 'win-signature',
  'LiverGuard_0.2.1_aarch64.dmg': 'ignored: not an updater package',
};
const base = { version: '0.2.1', tag: 'v0.2.1', repository: 'dailycoderlab/liver-guard', signatures, date };

test('lists both platforms with the release asset URLs and trimmed signatures', () => {
  assert.deepEqual(buildManifest(base), {
    version: '0.2.1',
    pub_date: '2026-11-11T09:00:00.000Z',
    platforms: {
      'darwin-aarch64': {
        signature: 'mac-signature',
        url: 'https://github.com/dailycoderlab/liver-guard/releases/download/v0.2.1/LiverGuard.app.tar.gz',
      },
      'windows-x86_64': {
        signature: 'win-signature',
        url: 'https://github.com/dailycoderlab/liver-guard/releases/download/v0.2.1/LiverGuard_0.2.1_x64-setup.exe',
      },
    },
  });
});

test('includes notes only when given', () => {
  assert.equal(buildManifest(base).notes, undefined);
  assert.equal(buildManifest({ ...base, notes: 'Fixes' }).notes, 'Fixes');
});

test('accepts a beta version', () => {
  assert.equal(buildManifest({ ...base, version: '0.3.0-beta.1', tag: 'v0.3.0-beta.1' }).version, '0.3.0-beta.1');
});

test('fails when a platform has no signed package', () => {
  const { 'LiverGuard.app.tar.gz': _mac, ...withoutMac } = signatures;
  assert.throws(() => buildManifest({ ...base, signatures: withoutMac }), /darwin-aarch64.*none/);
  const { 'LiverGuard_0.2.1_x64-setup.exe': _win, ...withoutWin } = signatures;
  assert.throws(() => buildManifest({ ...base, signatures: withoutWin }), /windows-x86_64.*none/);
});

test('fails on an ambiguous package or an empty signature', () => {
  assert.throws(() => buildManifest({ ...base, signatures: { ...signatures, 'Other.app.tar.gz': 'x' } }), /darwin-aarch64.*found 2/);
  assert.throws(() => buildManifest({ ...base, signatures: { ...signatures, 'LiverGuard.app.tar.gz': ' \n' } }), /signature .* is empty/);
});

test('fails on a tag that does not match, or a bad version or repository', () => {
  assert.throws(() => buildManifest({ ...base, tag: 'v0.2.2' }), /does not match/);
  assert.throws(() => buildManifest({ ...base, version: 'latest', tag: 'vlatest' }), /Not a version/);
  assert.throws(() => buildManifest({ ...base, repository: 'no-slash' }), /Not a repository/);
});

test('the command line reads .sig files next to the packages and writes the manifest', () => {
  const dir = mkdtempSync(join(tmpdir(), 'lg-manifest-'));
  writeFileSync(join(dir, 'LiverGuard.app.tar.gz'), 'tar');
  writeFileSync(join(dir, 'LiverGuard.app.tar.gz.sig'), 'mac-signature\n');
  writeFileSync(join(dir, 'LiverGuard_0.2.1_x64-setup.exe'), 'exe');
  writeFileSync(join(dir, 'LiverGuard_0.2.1_x64-setup.exe.sig'), 'win-signature\n');
  writeFileSync(join(dir, 'LiverGuard_0.2.1_aarch64.dmg'), 'dmg');
  const output = join(dir, 'latest.json');

  execFileSync(process.execPath, [SCRIPT, '--version', '0.2.1', '--tag', 'v0.2.1', '--repository', 'dailycoderlab/liver-guard', '--assets-dir', dir, '--output', output]);
  const manifest = JSON.parse(readFileSync(output, 'utf8'));
  assert.equal(manifest.platforms['darwin-aarch64'].signature, 'mac-signature');
  assert.equal(manifest.platforms['windows-x86_64'].signature, 'win-signature');

  const failed = spawnSync(process.execPath, [SCRIPT, '--version', '0.2.1', '--tag', 'v0.2.1', '--repository', 'dailycoderlab/liver-guard', '--assets-dir', mkdtempSync(join(tmpdir(), 'lg-empty-')), '--output', output]);
  assert.notEqual(failed.status, 0);
  assert.match(failed.stderr.toString(), /darwin-aarch64/);
});
