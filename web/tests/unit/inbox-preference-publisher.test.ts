import { it, expect } from 'vitest';
import { makeFixture } from '../e2e/harness/approved-signing.ts';
import {
  preparePreferenceSigningTransition,
  publicTransitionSnapshot
} from '../../src/lib/persistence/artifact-records.ts';
import {
  decodePublicRecord,
  publicRecordSnapshot
} from '../../src/lib/persistence/records.ts';
it('acknowledged preference signing marker preserves the complete original capture and cannot be reset', () => {
  const f = makeFixture(10050);
  try {
    const result = preparePreferenceSigningTransition(f.record, f.owner, f.id);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const next = publicTransitionSnapshot(result.value, f.owner, f.id)!;
    const decoded = decodePublicRecord(next.nextWire, f.owner, f.id);
    expect(decoded.ok).toBe(true);
    if (!decoded.ok) return;
    const original = publicRecordSnapshot(f.record, f.owner, f.id)!;
    const marked = publicRecordSnapshot(decoded.value, f.owner, f.id)!;
    expect(marked).toEqual({ ...original, revision: 1 });
    expect(
      preparePreferenceSigningTransition(decoded.value, f.owner, f.id).ok
    ).toBe(false);
  } finally {
    f.close();
  }
});
it('marker rejects food, withdrawal, foreign owner and forged handles', () => {
  for (const kind of [30402, 5] as const) {
    const f = makeFixture(kind);
    try {
      expect(
        preparePreferenceSigningTransition(f.record, f.owner, f.id).ok
      ).toBe(false);
    } finally {
      f.close();
    }
  }
  const f = makeFixture(10050);
  try {
    expect(
      preparePreferenceSigningTransition(f.record, '0'.repeat(64), f.id).ok
    ).toBe(false);
    expect(
      preparePreferenceSigningTransition({} as never, f.owner, f.id).ok
    ).toBe(false);
  } finally {
    f.close();
  }
});
