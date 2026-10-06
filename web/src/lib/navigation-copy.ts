import { decodeProductReference } from './nostr/references.ts';
// Explicit click callers alone use this public, hint-free coordinate action.
export async function copyProductReference(
  value: unknown,
  origin: unknown
): Promise<boolean> {
  const reference = decodeProductReference(value);
  if (!reference || typeof origin !== 'string' || origin.length > 2048)
    return false;
  try {
    const host = new URL(origin);
    if (
      host.origin !== origin ||
      host.username ||
      host.password ||
      (host.protocol !== 'https:' &&
        !(
          host.protocol === 'http:' &&
          ['127.0.0.1', 'localhost', '[::1]'].includes(host.hostname)
        ))
    )
      return false;
    await navigator.clipboard.writeText(
      `${host.origin}/products/${reference.naddr}`
    );
    return true;
  } catch {
    return false;
  }
}
