import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const scripts = path.dirname(fileURLToPath(import.meta.url));

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'otools-ai-switch-pack-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'dist'));
  fs.mkdirSync(path.join(root, 'lib'));
  fs.writeFileSync(path.join(root, 'dist/index.html'), '<html><script src="./assets/app.js"></script></html>');
  fs.mkdirSync(path.join(root, 'dist/assets'));
  fs.writeFileSync(path.join(root, 'dist/assets/app.js'), 'window.otools;');
  fs.writeFileSync(path.join(root, 'logo.svg'), '<svg xmlns="http://www.w3.org/2000/svg"/>');
  for (const name of ['LICENSE', 'THIRD_PARTY_NOTICES.md', 'UPSTREAM.md']) fs.writeFileSync(path.join(root, name), name);
  const manifest = {
    packid: 'otools-ai-switch', uuid: 'otools-ai-switch', version: '0.1.0',
    entry: 'dist/index.html', openMode: 'panel',
    devUrl: 'http://localhost:1420', quickDev: true,
    native: { enabled: true, libDir: 'lib', autoReload: true },
    autostart: { enabled: true, once: true, tasks: [{ command: 'start_route_proxy', args: {} }] },
  };
  fs.writeFileSync(path.join(root, 'plugin.json'), JSON.stringify(manifest));
  return { root, manifest };
}

function run(name, root) {
  return spawnSync(process.execPath, [path.join(scripts, name), '--root', root], { encoding: 'utf8' });
}

function addNative(root) {
  // A package fixture, not a loadable binary: native loading has its own smoke test.
  fs.writeFileSync(path.join(root, 'lib/Windows.dll'), 'MZ\0otools_plugin_invoke\0otools_plugin_free\0');
}

test('a frontend-only build cannot be shipped as the native plugin', (t) => {
  const { root } = fixture(t);
  const result = run('pack-plugin.mjs', root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /native library/i);
  assert.equal(fs.existsSync(path.join(root, 'dist-pack/otools-ai-switch-0.1.0.oplg')), false);
});

test('packaging strips development flags and produces a verifiable archive', (t) => {
  const { root } = fixture(t);
  addNative(root);
  const pack = run('pack-plugin.mjs', root);
  assert.equal(pack.status, 0, pack.stderr);
  const manifest = JSON.parse(fs.readFileSync(path.join(root, 'dist-pack/plugin.json')));
  assert.equal(manifest.devUrl, undefined);
  assert.equal(manifest.quickDev, undefined);
  assert.equal(manifest.native.autoReload, false);
  assert.equal(manifest.openMode, 'panel');
  assert.deepEqual(manifest.autostart, {
    enabled: true, once: true, tasks: [{ command: 'start_route_proxy', args: {} }],
  });
  const verify = run('verify-plugin.mjs', root);
  assert.equal(verify.status, 0, verify.stderr);
});

test('verification detects archive tampering', (t) => {
  const { root } = fixture(t);
  addNative(root);
  assert.equal(run('pack-plugin.mjs', root).status, 0);
  fs.appendFileSync(path.join(root, 'dist-pack/otools-ai-switch-0.1.0.oplg'), 'tampered');
  const result = run('verify-plugin.mjs', root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /SHA256/i);
});

test('packaging rejects missing or host-root frontend assets', (t) => {
  const { root } = fixture(t);
  addNative(root);
  fs.writeFileSync(path.join(root, 'dist/index.html'), '<script src="./assets/missing.js"></script>');
  let result = run('pack-plugin.mjs', root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /asset/i);
  fs.writeFileSync(path.join(root, 'dist/index.html'), '<script src="/assets/app.js"></script>');
  result = run('pack-plugin.mjs', root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /asset/i);
});

test('packaging refuses a path escaping the plugin root', (t) => {
  const { root, manifest } = fixture(t);
  addNative(root);
  manifest.entry = '../private.html';
  fs.writeFileSync(path.join(root, 'plugin.json'), JSON.stringify(manifest));
  const result = run('pack-plugin.mjs', root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /entry/i);
});
