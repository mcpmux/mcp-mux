#!/usr/bin/env node
/**
 * Set the app version across the three files release-please keeps in sync.
 * Used by CI to stamp a pre-release / promote version onto the working tree
 * before a Tauri build (these edits are transient and never committed).
 *
 * Patches:
 *   - apps/desktop/src-tauri/tauri.conf.json   $.version
 *   - Cargo.toml                               [workspace.package] version
 *   - apps/desktop/src-tauri/Cargo.toml        [package] version
 *
 * Usage: node scripts/set-version.mjs <version>
 *   e.g. node scripts/set-version.mjs 0.4.0-pre.318
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const version = process.argv[2];
if (!version) {
  console.error('error: missing <version> argument');
  process.exit(1);
}
// Accept SemVer X.Y.Z with an optional pre-release/build suffix.
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`error: '${version}' is not a valid semver version`);
  process.exit(1);
}

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');

/** Replace the first `version = "…"` line that appears under `[table]`. */
function setTomlTableVersion(path, table) {
  const src = readFileSync(path, 'utf8');
  const lines = src.split('\n');
  let inTable = false;
  let patched = false;
  for (let i = 0; i < lines.length; i++) {
    const header = lines[i].match(/^\s*\[([^\]]+)\]\s*$/);
    if (header) {
      inTable = header[1].trim() === table;
      continue;
    }
    if (inTable && /^\s*version\s*=/.test(lines[i])) {
      lines[i] = lines[i].replace(/version\s*=\s*"[^"]*"/, `version = "${version}"`);
      patched = true;
      break;
    }
  }
  if (!patched) throw new Error(`${path}: no version found under [${table}]`);
  writeFileSync(path, lines.join('\n'));
  console.log(`  ${path} [${table}] -> ${version}`);
}

/** Replace the top-level `"version"` key in a JSON file (text-level, format-preserving). */
function setJsonVersion(path) {
  const src = readFileSync(path, 'utf8');
  let patched = false;
  const out = src.replace(/("version"\s*:\s*")[^"]*(")/, (_, a, b) => {
    patched = true;
    return `${a}${version}${b}`;
  });
  if (!patched) throw new Error(`${path}: no "version" key found`);
  writeFileSync(path, out);
  console.log(`  ${path} $.version -> ${version}`);
}

console.log(`Setting version to ${version}`);
setJsonVersion(join(repoRoot, 'apps/desktop/src-tauri/tauri.conf.json'));
setTomlTableVersion(join(repoRoot, 'Cargo.toml'), 'workspace.package');
setTomlTableVersion(join(repoRoot, 'apps/desktop/src-tauri/Cargo.toml'), 'package');
