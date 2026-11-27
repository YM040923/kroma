import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pickLocalFolder } from './src/local-folder.ts';

// Only the host boundary is stubbed; no native window, HTTP call or grant is made.
test('a stale host without a picker reports unavailable, not cancellation', async () => {
  assert.equal(await pickLocalFolder({}), 'unavailable');
  assert.equal(await pickLocalFolder({ pickModuleFolder: true }), 'unavailable');
});

test('cancel returns no patch for the current mapping', async () => {
  assert.equal(await pickLocalFolder({ pickModuleFolder: async () => ({ selected: false }) }), null);
});

test('selection returns the new Windows display path separately from the grant', async () => {
  const displayPath = String.raw`D:\媒体库\115 Movies`;
  const guestPath = '/selected/test-folder';
  const patch = await pickLocalFolder({
    async pickModuleFolder(id) {
      assert.equal(id, 'tv.kroma.cloud115');
      return { selected: true, guestPath, displayPath };
    },
  });
  assert.deepEqual(patch, { local: guestPath, localDisplay: displayPath });
});

test('an incomplete success response does not replace the selection', async () => {
  const host = { pickModuleFolder: async () => ({ selected: true, displayPath: 'D:\\Movies' }) };
  assert.equal(await pickLocalFolder(host), 'unavailable');
});

test('the original HTTP error reaches the UI for SDK apiErrorText formatting', async () => {
  const error = Object.assign(new Error('POST failed (403)'), {
    body: { error: 'Open this page via localhost on the Windows server.' },
  });
  await assert.rejects(
    pickLocalFolder({ pickModuleFolder: () => Promise.reject(error) }),
    (reason) => reason === error,
  );
});
