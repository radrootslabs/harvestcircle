// Explicit prototype display policy; protocol order and signed timestamps stay
// unchanged. A caller supplies wall-clock seconds, independently of the
// monotonic clock used for bounded request deadlines.
export const FUTURE_QUARANTINE_SECONDS = 300;
export type WallClock = Readonly<{ nowSeconds: () => number }>;
export type FutureAssessment =
  'within_policy' | 'future_quarantined' | 'clock_unavailable';
export function wallClockAvailable(value: number): boolean {
  return (
    Number.isFinite(value) && value >= 0 && value <= Number.MAX_SAFE_INTEGER
  );
}
export function assessFutureTimestamp(
  createdAt: number,
  nowSeconds: number
): FutureAssessment {
  if (!wallClockAvailable(nowSeconds)) return 'clock_unavailable';
  if (!Number.isSafeInteger(createdAt) || createdAt < 0)
    throw new Error('food_head_timestamp_invalid');
  // Subtract rather than adding skew to a potentially maximum-safe timestamp.
  return createdAt - nowSeconds > FUTURE_QUARANTINE_SECONDS
    ? 'future_quarantined'
    : 'within_policy';
}
