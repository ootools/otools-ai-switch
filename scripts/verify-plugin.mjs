import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { option, readZip, validateManifest, validateEntries } from './package-utils.mjs';

try {
  const root = path.resolve(option('root', path.join(path.dirname(fileURLToPath(import.meta.url)), '..')));
  const directory = path.resolve(root, option('out', 'dist-pack'));
  const manifestBytes = fs.readFileSync(path.join(directory, 'plugin.json'));
  const manifest = JSON.parse(manifestBytes);
  validateManifest(manifest);
  if (manifest.devUrl || manifest.quickDev || manifest.native.autoReload) throw new Error('Development settings in production package');
  const filename = `${manifest.packid}-${manifest.version}.oplg`;
  const requiredSums = new Set([filename, 'plugin.json', 'logo.svg']);
  for (const line of fs.readFileSync(path.join(directory, 'SHA256SUMS'), 'utf8').trim().split(/\r?\n/)) {
    const match = /^([a-f0-9]{64})  (.+)$/.exec(line);
    if (!match || !requiredSums.delete(match[2])) throw new Error('Invalid SHA256 manifest');
    const actual = createHash('sha256').update(fs.readFileSync(path.join(directory, match[2]))).digest('hex');
    if (actual !== match[1]) throw new Error(`SHA256 mismatch: ${match[2]}`);
  }
  if (requiredSums.size) throw new Error('Incomplete SHA256 manifest');
  const entries = readZip(fs.readFileSync(path.join(directory, filename)));
  if (!entries.get('plugin.json')?.equals(manifestBytes)) throw new Error('Packaged manifest mismatch');
  if (!entries.get('logo.svg')?.equals(fs.readFileSync(path.join(directory, 'logo.svg')))) throw new Error('Packaged icon mismatch');
  const libraries = validateEntries(manifest, entries);
  for (const name of entries.keys()) {
    if (!/^(plugin\.json|logo\.svg|LICENSE|THIRD_PARTY_NOTICES\.md|UPSTREAM\.md|dist\/.*|lib\/.*|LICENSES\/.*|resources\/.*)$/.test(name)) {
      throw new Error(`Unexpected packaged file: ${name}`);
    }
  }
  console.log(`Verified ${filename}: ${entries.size} files, CRC and SHA256 valid, native ${libraries.join(', ')}, panel mode`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
