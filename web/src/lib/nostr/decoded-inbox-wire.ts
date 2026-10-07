import { PUBLIC_INGRESS_BUDGETS } from '../config/budgets.ts';

// The qualified SDK exposes parsed JSON, not original transport text. Preserve
// every decoded JSON field without invoking getters, toJSON or SDK symbols.
// Bounds are post-parse logical work; this makes no allocation/preparse claim.
export function decodedInboxWire(
  input: unknown,
  assertActive: () => void
): string | undefined {
  const parts: string[] = [];
  const encoder = new TextEncoder();
  let bytes = 0,
    nodes = 0;
  function append(text: string) {
    bytes += encoder.encode(text).length;
    if (bytes > PUBLIC_INGRESS_BUDGETS.eventBytes)
      throw new Error('candidate_invalid');
    parts.push(text);
  }
  function inspect(
    value: object,
    name: string
  ): PropertyDescriptor | undefined {
    assertActive();
    const descriptor = Object.getOwnPropertyDescriptor(value, name);
    assertActive();
    return descriptor;
  }
  function data(value: object, name: string): unknown {
    const descriptor = inspect(value, name);
    if (!descriptor || !('value' in descriptor))
      throw new Error('candidate_invalid');
    return descriptor.value;
  }
  function visit(value: unknown): void {
    assertActive();
    if (++nodes > PUBLIC_INGRESS_BUDGETS.eventBytes)
      throw new Error('candidate_invalid');
    if (
      value === null ||
      typeof value === 'boolean' ||
      typeof value === 'number'
    ) {
      if (typeof value === 'number' && !Number.isFinite(value))
        throw new Error('candidate_invalid');
      append(JSON.stringify(value));
    } else if (typeof value === 'string') {
      if (value.length > PUBLIC_INGRESS_BUDGETS.eventBytes)
        throw new Error('candidate_invalid');
      append(JSON.stringify(value));
    } else if (typeof value === 'object') {
      if (Array.isArray(value)) {
        const length = data(value, 'length');
        if (
          typeof length !== 'number' ||
          !Number.isSafeInteger(length) ||
          length < 0 ||
          length > PUBLIC_INGRESS_BUDGETS.eventBytes
        )
          throw new Error('candidate_invalid');
        append('[');
        for (let index = 0; index < length; index++) {
          if (index) append(',');
          visit(data(value, String(index)));
        }
        append(']');
      } else {
        append('{');
        let index = 0;
        for (const name in value) {
          assertActive();
          const descriptor = inspect(value, name);
          if (!descriptor) continue;
          if (index >= PUBLIC_INGRESS_BUDGETS.eventBytes)
            throw new Error('candidate_invalid');
          if (index) append(',');
          if (name.length > PUBLIC_INGRESS_BUDGETS.eventBytes)
            throw new Error('candidate_invalid');
          append(JSON.stringify(name));
          append(':');
          visit(data(value, name));
          index++;
        }
        append('}');
      }
    } else throw new Error('candidate_invalid');
  }
  try {
    if (!input || typeof input !== 'object' || Array.isArray(input))
      return undefined;
    visit(input);
    return parts.join('');
  } catch {
    return undefined;
  }
}
