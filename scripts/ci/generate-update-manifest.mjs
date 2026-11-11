// Builds the `latest.json` the in-app updater reads (Tauri updater format, static manifest).
//
//   node scripts/ci/generate-update-manifest.mjs \
//     --version 0.2.1 --tag v0.2.1 --repository dailycoderlab/liver-guard \
//     --assets-dir dist --output dist/latest.json [--notes "..."]
//
// It looks in --assets-dir for the updater packages that `tauri build` writes when
// `bundle.createUpdaterArtifacts` is on, each next to its `.sig` signature file:
//
//   darwin-aarch64   *.app.tar.gz      (the .app inside a tarball)
//   windows-x86_64   *-setup.exe       (the NSIS installer itself)
//
// A platform whose package or signature is missing fails the run, so a release is never
// published with an update feed that silently skips a platform.

import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

/** Platform keys of the Tauri updater, and which file in the assets directory is its package. */
export const PLATFORMS = [
  { key: 'darwin-aarch64', match: (name) => name.endsWith('.app.tar.gz') },
  { key: 'windows-x86_64', match: (name) => name.endsWith('-setup.exe') },
];

const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;

/**
 * @param {object} input
 * @param {string} input.version      without the leading "v"
 * @param {string} input.tag          the Git tag the assets are attached to
 * @param {string} input.repository   "owner/name"
 * @param {Record<string, string>} input.signatures  asset file name -> content of its `.sig` file
 * @param {string} [input.notes]
 * @param {Date} [input.date]
 */
export function buildManifest({ version, tag, repository, signatures, notes, date = new Date() }) {
  if (!SEMVER.test(version)) throw new Error(`Not a version: "${version}"`);
  if (tag !== `v${version}`) throw new Error(`Tag ${tag} does not match version ${version}`);
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error(`Not a repository: "${repository}"`);

  const platforms = {};
  for (const { key, match } of PLATFORMS) {
    const names = Object.keys(signatures).filter(match);
    if (names.length !== 1) {
      throw new Error(`${key}: expected exactly one updater package with a signature, found ${names.length} (${names.join(', ') || 'none'})`);
    }
    const [name] = names;
    const signature = signatures[name].trim();
    if (!signature) throw new Error(`${key}: the signature of ${name} is empty`);
    platforms[key] = {
      signature,
      url: `https://github.com/${repository}/releases/download/${tag}/${encodeURIComponent(name)}`,
    };
  }

  const manifest = { version, pub_date: date.toISOString(), platforms };
  if (notes) manifest.notes = notes;
  return manifest;
}

/** Reads `<name>` and `<name>.sig` pairs from a directory. */
export function readSignatures(dir) {
  const files = new Set(readdirSync(dir));
  const signatures = {};
  for (const file of files) {
    if (file.endsWith('.sig')) continue;
    if (files.has(`${file}.sig`)) signatures[file] = readFileSync(join(dir, `${file}.sig`), 'utf8');
  }
  return signatures;
}

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    if (!flag?.startsWith('--') || argv[i + 1] === undefined) throw new Error(`Bad argument near "${flag ?? ''}"`);
    args[flag.slice(2)] = argv[i + 1];
  }
  for (const required of ['version', 'tag', 'repository', 'assets-dir', 'output']) {
    if (!args[required]) throw new Error(`Missing --${required}`);
  }
  return args;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = parseArgs(process.argv.slice(2));
    const manifest = buildManifest({
      version: args.version,
      tag: args.tag,
      repository: args.repository,
      signatures: readSignatures(args['assets-dir']),
      notes: args.notes,
    });
    writeFileSync(args.output, `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(`Wrote ${args.output}: ${Object.keys(manifest.platforms).join(', ')}`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
