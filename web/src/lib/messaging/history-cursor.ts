// These detached scalars guide bounded outer-event queries only. They confer
// no owner, decrypt, storage or transport permission. Inner message times never
// enter this plan. No durable cursor/schema or complete-inbox claim is minted.
export function outerHistoryPlan(now: unknown, previousOuterCheck?: unknown) {
  const time = (value: unknown): number => {
    if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0)
      throw Error('outer_check_time_invalid');
    return value;
  };
  const clock = time(now),
    previous =
      previousOuterCheck === undefined ? clock : time(previousOuterCheck);
  return freezePlan(
    Math.max(0, Math.min(clock, previous) - (48 * 60 * 60 + 5 * 60))
  );
}
function freezePlan(since: number) {
  const latest = {},
    overlap = { since: since };
  Object.freeze(latest);
  Object.freeze(overlap);
  const plan = {
    latest: latest,
    overlap: overlap,
    historyComplete: false as const,
    olderRecoveryRequired: true as const
  };
  return Object.freeze(plan);
}
