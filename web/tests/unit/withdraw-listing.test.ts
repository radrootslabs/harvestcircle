import { expect, test } from 'vitest';
import { reviewOwnedWithdrawal } from '../../src/lib/publishing/withdraw-listing.ts';
import type { OwnedEditSource } from '../../src/lib/publishing/edit-source.ts';
import type { PublicDraftController } from '../../src/lib/publishing/draft-controller.ts';
import { makeFixture } from '../e2e/harness/approved-signing.ts';
import { runCapturedPublicEffect } from '../../src/lib/runtime/effect-ownership.ts';
import type { PublicQuotaRepository } from '../../src/lib/persistence/quota.ts';

for (const source of [undefined, null, {}, { owner: 'copied' }])
  test(`withdrawal cannot acquire source custody from ${JSON.stringify(source)}`, async () => {
    const result = await reviewOwnedWithdrawal(
      source as OwnedEditSource,
      {} as PublicDraftController,
      '00000000-0000-4000-8000-000000000120',
      'reviewed_owned_withdrawal'
    );
    expect(result).toEqual({ status: 'blocked', reason: 'invalid_scope' });
  });

for (const kind of [30402, 5, 10050] as const)
  test(`withdrawal signing mode admits only linked kind5 and never invokes SSR effects (${kind})`, async () => {
    const fixture = makeFixture(kind);
    let calls = 0;
    try {
      const result = await runCapturedPublicEffect(
        {} as PublicQuotaRepository,
        fixture.record,
        fixture.id,
        { owner: fixture.owner, session: Symbol(), current: () => true },
        'reviewed_prepared_withdrawal_operation',
        () => {
          calls++;
          return Promise.resolve();
        }
      );
      expect(result.status).toBe(kind === 5 ? 'unavailable' : 'invalid');
      expect(calls).toBe(0);
    } finally {
      fixture.close();
    }
  });

test('linked removal cannot enter the prepared ordinary-listing signing mode', async () => {
  const fixture = makeFixture(5);
  let calls = 0;
  try {
    const result = await runCapturedPublicEffect(
      {} as PublicQuotaRepository,
      fixture.record,
      fixture.id,
      { owner: fixture.owner, session: Symbol(), current: () => true },
      'reviewed_prepared_listing_operation',
      () => {
        calls++;
        return Promise.resolve();
      }
    );
    expect(result.status).toBe('invalid');
    expect(calls).toBe(0);
  } finally {
    fixture.close();
  }
});
