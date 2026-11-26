import test from 'node:test';
import assert from 'node:assert/strict';
import { credential, marketPayload, publishMarket, releaseMeta, productionManifest, releasePlan, validateAuthConfig } from './publish-market.mjs';
import { expectedPackageNames } from './platform-packages.mjs';

const response = (data, ok = true) => ({ ok, status: ok ? 200 : 403, json: async () => data });
const manifest = { packid: 'plugin-a', uuid: 'plugin-a', version: '1.0.0', displayName: 'A', entry: 'dist/index.html', minOtoolsVersion: '0.9.0', native: { enabled: true, libDir: 'lib', autoReload: true } };

test('OIDC exchange stays in one job', async () => {
  const calls = [];
  const token = await credential('oidc', {
    XYCLOUD_OIDC_ISSUER: 'https://api.test/token', XYCLOUD_PUBLISHER_ID: 'grant1',
    ACTIONS_ID_TOKEN_REQUEST_URL: 'https://github.test/token?a=1', ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-secret',
  }, async (url, options) => {
    calls.push({ url: String(url), options });
    return calls.length === 1 ? response({ value: 'signed.github.jwt' }) : response({ code: 200, data: { access_token: 'xy_access_short' } });
  }, () => {});
  assert.equal(token, 'xy_access_short');
  assert.match(calls[0].url, /audience=otools-ci/);
  assert.equal(calls[1].options.headers.Authorization, 'Bearer signed.github.jwt');
});

test('PAT, login and shared secret are explicit independent modes', async () => {
  for (const [mode, key, value] of [['pat', 'XYCLOUD_PAT', 'xy_pat_test'], ['login', 'XYCLOUD_LOGIN_TOKEN', 'Bearer session-jwt'], ['shared', 'OTOOLS_MARKET_TOKEN', 'official-secret']]) {
    assert.equal(await credential(mode, { [key]: value }, () => { throw Error('no exchange'); }, () => {}), value.replace(/^Bearer /, ''));
  }
  await assert.rejects(credential('oidc', { OTOOLS_MARKET_TOKEN: 'must-not-fallback' }, () => {}, () => {}));
});

test('HTTP and application errors fail rather than claiming success', async () => {
  await assert.rejects(publishMarket('https://api.test/publish', 'xy_pat_test', {}, async () => response({ code: 403 })));
  await assert.rejects(publishMarket('https://api.test/publish', 'xy_pat_test', {}, async () => response({}, false)));
  await assert.rejects(publishMarket('https://api.test/publish', 'xy_pat_test', {}, async () => response({ error: 'unexpected' })));
});

test('combined payload keeps packageUrl and reports packageMode=0', () => {
  const p = marketPayload(manifest, 'owner/repo', 'v1.0.0', '1.0.0');
  assert.equal(p.packageUrl, 'https://github.com/owner/repo/releases/download/v1.0.0/plugin-a-1.0.0.oplg');
  assert.equal(p.packageMode, 0);
  assert.equal(p.logo, 'https://raw.githubusercontent.com/owner/repo/v1.0.0/logo.svg');
  assert.equal(p.minOToolsVersion, '0.9.0');
  assert.equal(p.minOtoolsVersion, undefined);
  assert.equal(p.supportWindows, true);
  assert.equal(p.supportMacos, true);
  assert.equal(p.supportLinux, true);
  assert.deepEqual(p.screenshots, []);
  assert.equal(p.native.autoReload, false);
});

test('three-platform payload exposes three URLs and keeps the combined fallback', () => {
  const p = marketPayload(manifest, 'owner/repo', 'v1.0.0', '1.0.0', { platformPackages: true });
  const base = 'https://github.com/owner/repo/releases/download/v1.0.0/plugin-a-1.0.0';
  assert.equal(p.packageMode, 1);
  assert.equal(p.packageUrl, `${base}.oplg`);
  assert.equal(p.packageUrlWindows, `${base}-windows.oplg`);
  assert.equal(p.packageUrlLinux, `${base}-linux.oplg`);
  assert.equal(p.packageUrlMacos, `${base}-macos.oplg`);
  assert.deepEqual(expectedPackageNames('plugin-a', '1.0.0', true), [
    'plugin-a-1.0.0-windows.oplg', 'plugin-a-1.0.0-linux.oplg', 'plugin-a-1.0.0-macos.oplg', 'plugin-a-1.0.0.oplg',
  ]);
});

test('screenshots are rewritten to tag-pinned raw URLs', () => {
  const p = marketPayload({ ...manifest, screenshots: ['screenshots/a.webp', './screenshots/b.webp', '', 'https://cdn.test/c.webp'] }, 'owner/repo', 'v1.0.0', '1.0.0');
  assert.deepEqual(p.screenshots, [
    'https://raw.githubusercontent.com/owner/repo/v1.0.0/screenshots/a.webp',
    'https://raw.githubusercontent.com/owner/repo/v1.0.0/screenshots/b.webp',
    'https://cdn.test/c.webp',
  ]);
});

test('version, tag, packid and entry inputs are validated', () => {
  assert.throws(() => releaseMeta({ packid: 'a', version: '1.0.0' }, '1.0.0; echo bad', ''));
  assert.throws(() => releaseMeta({ packid: 'a', version: '1.0.0' }, '', 'v2.0.0'));
  assert.equal(releaseMeta({ packid: 'a', version: '1.0.0' }, '', 'v1.0.0').version, '1.0.0');
  assert.throws(() => releaseMeta({ version: '1.0.0' }));
  assert.throws(() => productionManifest({ packid: 'a', version: '1.0.0', entry: '../outside' }));
});

test('dry-run cannot authorize writes and 1/0 switches are accepted', () => {
  assert.deepEqual(releasePlan({ EVENT_NAME: 'workflow_dispatch', DRY_RUN: 'true', PUBLISH_MARKET: 'true' }),
    { publish_enabled: false, market_requested: true, market_enabled: false, validate_release: true });
  assert.equal(releasePlan({ EVENT_NAME: 'push', RELEASE_SOURCE_TAG: 'v1.0.0', AUTO_PUBLISH_MARKET: '1' }).market_enabled, true);
  assert.equal(releasePlan({ EVENT_NAME: 'workflow_dispatch', DRY_RUN: 'false', PUBLISH_MARKET: '0' }).market_enabled, false);
  assert.throws(() => releasePlan({ EVENT_NAME: 'pull_request' }));
  assert.throws(() => releasePlan({ EVENT_NAME: 'push', RELEASE_SOURCE_TAG: 'main' }));
});

test('credentials are preflighted without network calls or echoing secrets', () => {
  assert.doesNotThrow(() => validateAuthConfig('pat', { XYCLOUD_PAT: 'xy_pat_' + 'a'.repeat(64) }));
  assert.throws(() => validateAuthConfig('pat', { XYCLOUD_PAT: 'github-token' }));
  assert.throws(() => validateAuthConfig('oidc', { XYCLOUD_OIDC_ISSUER: 'http://unsafe.test', XYCLOUD_PUBLISHER_ID: 'id' }));
});
