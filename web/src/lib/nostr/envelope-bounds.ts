// Numeric preflight only: these values are not signature or profile admission.
export function safeUnsignedInteger(value: unknown): number | undefined {
  if (
    typeof value !== 'number' ||
    !Number.isSafeInteger(value) ||
    value < 0 ||
    (value === 0 && 1 / value === -Infinity)
  )
    return undefined;
  return value;
}

export type BoundedEnvelopeNumbers = Readonly<{
  kind: number;
  created_at: number;
}>;
export function boundedEnvelopeNumbers(
  kindInput: unknown,
  timestampInput: unknown
): BoundedEnvelopeNumbers | undefined {
  const kind = safeUnsignedInteger(kindInput);
  const createdAt = safeUnsignedInteger(timestampInput);
  // Frozen NIP-01 kind range; the browser's timestamp envelope is narrower
  // than the public Rust u64 type. Unsupported numbers never get rounded.
  if (kind === undefined || kind > 65535 || createdAt === undefined)
    return undefined;
  return { kind, created_at: createdAt };
}

export function foodPublishedAt(value: unknown): number | undefined {
  // Public FoodPublishedAt requires a positive canonical unsigned integer.
  // Compare the ASCII magnitude before Number conversion, so u64 values
  // outside the exact JavaScript range cannot be rounded into trusted data.
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    value.length > 16 ||
    !/^[1-9][0-9]*$/u.test(value) ||
    (value.length === 16 && value > '9007199254740991')
  )
    return undefined;
  return Number(value);
}
