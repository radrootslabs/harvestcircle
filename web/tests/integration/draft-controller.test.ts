import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createPublicDraftController,
  flushPublicDraft,
  editPublicDraft,
  type PublicDraftController
} from '../../src/lib/publishing/draft-controller.ts';
import type { PublicDraftRepository } from '../../src/lib/persistence/drafts.ts';
await test('actual autosave factory rejects a fabricated owner without opening storage or acquiring private input', async () => {
  const repository = Object.freeze({}) as PublicDraftRepository;
  const privateInput = {
    get body() {
      throw new Error('must not read private input');
    }
  };
  assert.equal(
    await createPublicDraftController(repository, privateInput),
    undefined
  );
  const controller = Object.freeze({}) as PublicDraftController;
  assert.equal(editPublicDraft(controller, privateInput), false);
  assert.equal(await flushPublicDraft(controller), false);
});
