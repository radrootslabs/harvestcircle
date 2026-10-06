// Reviewed scalar history capability. Import is SSR-pure; callers own the
// current public query/generation and render/disposal lifetime. No DOM handle,
// callable browser method, timer, storage or global object leaves this module.
function validPosition(value: unknown): value is number {
  return (
    typeof value === 'number' &&
    Number.isFinite(value) &&
    value >= 0 &&
    value <= Number.MAX_SAFE_INTEGER
  );
}
export function captureSearchScroll(): number | undefined {
  if (typeof window === 'undefined') return undefined;
  const position = window.scrollY;
  return validPosition(position) ? position : undefined;
}
export function restoreSearchScroll(position: unknown): void {
  if (typeof window === 'undefined' || !validPosition(position)) return;
  window.scrollTo(0, position);
}
