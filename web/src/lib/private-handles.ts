// Local handles identify records; callers must separately resolve the active owner.
export function canonicalLocalId(value: unknown): string | undefined {
  return typeof value === 'string' &&
    value.length === 36 &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      value
    )
    ? value
    : undefined;
}

// Explicit local creation only. No import/render operation creates a handle.
export function newLocalId(): string | undefined {
  try {
    return canonicalLocalId(globalThis.crypto.randomUUID());
  } catch {
    return undefined;
  }
}
