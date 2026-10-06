import { describe, expect, it } from 'vitest';
import {
  createPublicDraftRepository,
  createPublicDraft,
  readPublicDraft,
  savePublicDraft,
  listPublicDrafts,
  type PublicDraftRepository
} from '../../src/lib/persistence/drafts.ts';

describe('public draft repository capability admission', () => {
  it('a fabricated database cannot mint a repository for a valid owner', () => {
    expect(
      createPublicDraftRepository(
        { kind: 'browser_database' },
        '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
      )
    ).toBeUndefined();
  });
  it('foreign or fabricated repository objects cannot acquire storage or form getters', async () => {
    const fake = Object.freeze({}) as PublicDraftRepository;
    const form = {
      get title() {
        throw new Error('must not acquire caller data');
      }
    };
    const outcomes = await Promise.all([
      createPublicDraft(fake, form),
      readPublicDraft(fake, 'not-an-id'),
      savePublicDraft(fake, 'not-an-id', 0, form),
      listPublicDrafts(fake)
    ]);
    expect(outcomes).toEqual(
      Array(4).fill({ ok: false, reason: 'invalid_scope' })
    );
  });
  it('unknown owners cannot open an implicit database', () => {
    for (const owner of [undefined, null, '', '0'.repeat(64), 'npub1invalid'])
      expect(
        createPublicDraftRepository({ kind: 'browser_database' }, owner)
      ).toBeUndefined();
  });
});
