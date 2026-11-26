import test from 'node:test';
import assert from 'node:assert/strict';
import { assertReleaseTarget } from './release-policy.mjs';

const json = (data, status = 200) => ({ ok: status >= 200 && status < 300, status, json: async () => data });

test('a missing tag is allowed for a first release', async () => {
  await assertReleaseTarget('owner/repo', 'v1.0.0', 'a'.repeat(40), 'gh_token', async () => json({}, 404));
});

test('an annotated tag pointing at the build commit is accepted', async () => {
  const urls = [];
  await assertReleaseTarget('owner/repo', 'v1.0.0', 'a'.repeat(40), 'gh_token', async (url) => {
    urls.push(String(url));
    return urls.length === 1
      ? json({ object: { type: 'tag', sha: 'b'.repeat(40) } })
      : json({ object: { type: 'commit', sha: 'a'.repeat(40) } });
  });
  assert.match(urls[0], /\/ref\/tags\/v1\.0\.0$/);
  assert.match(urls[1], /\/git\/tags\//);
});

test('a tag pointing at another commit is rejected', async () => {
  await assert.rejects(
    assertReleaseTarget('owner/repo', 'v1.0.0', 'a'.repeat(40), 'gh_token', async () => json({ object: { type: 'commit', sha: 'c'.repeat(40) } })),
    /其他提交/,
  );
});

test('invalid inputs and HTTP failures are rejected', async () => {
  await assert.rejects(assertReleaseTarget('bad repo', 'v1.0.0', 'a'.repeat(40), 'gh_token', async () => json({})));
  await assert.rejects(assertReleaseTarget('owner/repo', 'v1.0.0', 'short', 'gh_token', async () => json({})));
  await assert.rejects(assertReleaseTarget('owner/repo', 'v1.0.0', 'a'.repeat(40), '', async () => json({})));
  await assert.rejects(assertReleaseTarget('owner/repo', 'v1.0.0', 'a'.repeat(40), 'gh_token', async () => json({}, 500)));
});
