import { describe, expect, it } from 'vitest';
import { makeFixture } from '../e2e/harness/approved-signing.ts';
import {
  runCapturedPublicEffect,
  publicEffectSnapshot,
  type PublicEffectLease
} from '../../src/lib/runtime/effect-ownership.ts';
import type { PublicQuotaRepository } from '../../src/lib/persistence/quota.ts';
describe('owned effect admission boundaries', () => {
  it('keeps genuine browser-only capture unavailable in SSR without invoking work', async () => {
    const f = makeFixture();
    let calls = 0;
    try {
      const result = await runCapturedPublicEffect(
        {} as PublicQuotaRepository,
        f.record,
        f.id,
        { owner: f.owner, session: Symbol(), current: () => true },
        'reviewed_captured_operation',
        () => {
          calls++;
          return Promise.resolve('forbidden');
        }
      );
      expect(result.status).toBe('unavailable');
      expect(calls).toBe(0);
    } finally {
      f.close();
    }
  });
  it('rejects unreviewed or forged and cross-ID capture before invoking effects', async () => {
    const f = makeFixture();
    let calls = 0;
    try {
      const context = {
          owner: f.owner,
          session: Symbol(),
          current: () => true
        },
        work = () => {
          calls++;
          return Promise.resolve();
        };
      expect(
        (
          await runCapturedPublicEffect(
            {} as PublicQuotaRepository,
            f.record,
            f.id,
            context,
            undefined,
            work
          )
        ).status
      ).toBe('invalid');
      expect(
        (
          await runCapturedPublicEffect(
            {} as PublicQuotaRepository,
            {} as never,
            f.id,
            context,
            'reviewed_captured_operation',
            work
          )
        ).status
      ).toBe('invalid');
      expect(
        (
          await runCapturedPublicEffect(
            {} as PublicQuotaRepository,
            f.record,
            crypto.randomUUID(),
            context,
            'reviewed_captured_operation',
            work
          )
        ).status
      ).toBe('invalid');
      expect(calls).toBe(0);
    } finally {
      f.close();
    }
  });
  it('fabricated ownership leases have no capability or mutable snapshot', () => {
    expect(publicEffectSnapshot({} as PublicEffectLease)).toBeUndefined();
  });
});
