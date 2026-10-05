import { canonicalFoodAmount, type CanonicalFoodAmount } from './values.ts';
const encoder = new TextEncoder();
export function boundedUtf8(value: string, maximum: number): boolean {
  return (
    value.length <= maximum &&
    value.isWellFormed() &&
    encoder.encode(value).length <= maximum
  );
}
export function foodTextValid(value: string): boolean {
  return (
    value.length > 0 &&
    boundedUtf8(value, 4096) &&
    !/[\p{Cc}\p{Cf}]/u.test(value) &&
    !/^\p{White_Space}|\p{White_Space}$/u.test(value)
  );
}
export function foodIdentifierValid(value: string): boolean {
  return (
    value.length > 0 &&
    boundedUtf8(value, 512) &&
    !/[\p{White_Space}\p{Cc}\p{Cf}]/u.test(value)
  );
}
export function foodContentPresent(value: string): boolean {
  // Rust contract whitespace also includes U+001C..U+001F.
  return Array.from(value).some(
    (character) =>
      !/\p{White_Space}/u.test(character) &&
      !(character.codePointAt(0)! >= 28 && character.codePointAt(0)! <= 31)
  );
}
export function incomingAmount(value: string): CanonicalFoodAmount | undefined {
  if (
    value.length === 0 ||
    value.length > 29 ||
    !/^[0-9]+(?:\.[0-9]+)?$/u.test(value)
  )
    return undefined;
  if (value.length - (value.includes('.') ? 1 : 0) > 28) return undefined;
  const [integer, fraction] = value.split('.');
  let start = 0;
  while (start < integer.length && integer[start] === '0') start++;
  const whole = integer.slice(start) || '0';
  let end = fraction?.length ?? 0;
  while (end > 0 && fraction?.[end - 1] === '0') end--;
  const tail = fraction?.slice(0, end);
  return canonicalFoodAmount(tail ? `${whole}.${tail}` : whole);
}
