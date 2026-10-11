import { readRelayPolicy, type RelayPolicy } from './relays.ts';

// Static availability is read only from original admitted policy. This boolean
// is not an owner, signer, route, lease or permission to replay retained work.
export function operationEnabled(
  policy: RelayPolicy | undefined,
  operation: unknown
): boolean {
  if (!policy) return false;
  try {
    const manifest = readRelayPolicy(policy);
    if (operation === 'posting') return manifest.postingEnabled;
    if (operation === 'messaging') return manifest.messagingEnabled;
    if (operation === 'new_listing')
      return manifest.postingEnabled && manifest.messagingEnabled;
    return false;
  } catch {
    return false;
  }
}
