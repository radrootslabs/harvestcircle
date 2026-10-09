import { test, expect } from 'vitest';
import { outerHistoryPlan } from '../../src/lib/messaging/history-cursor.ts';
test('latest window has no inner-time cursor and overlap covers two-day backdate plus five minutes', () => {
  const p = outerHistoryPlan(1000000);
  expect(p.latest).toEqual({});
  expect(p.overlap).toEqual({ since: 826900 });
  expect(p.historyComplete).toBe(false);
  expect(p.olderRecoveryRequired).toBe(true);
});
test('previous outer check widens reconnect overlap instead of using latest inner time', () =>
  expect(outerHistoryPlan(1000000, 900000).overlap).toEqual({ since: 726900 }));
test('backward clock change uses earlier actual clock instead of future persisted check', () =>
  expect(outerHistoryPlan(900000, 1000000).overlap).toEqual({ since: 726900 }));
test('early clock clamps overlap at zero without negative Nostr time', () =>
  expect(outerHistoryPlan(100).overlap).toEqual({ since: 0 }));
test('noninteger negative nonfinite and forged object clocks or hints reject', () => {
  for (const t of [-1, 1.5, Infinity, NaN, {}, '100']) {
    expect(() => outerHistoryPlan(t)).toThrow();
    expect(() => outerHistoryPlan(1000000, t)).toThrow();
  }
});
test('immutable detached plan cannot narrow later overlap', () => {
  const p = outerHistoryPlan(1000000);
  expect(Object.isFrozen(p)).toBe(true);
  expect(Object.isFrozen(p.overlap)).toBe(true);
  expect(outerHistoryPlan(1000000).overlap.since).toBe(826900);
});
