import {
  createExtensionAdapter,
  extensionSnapshot,
  connectExtensionAdapter,
  recheckExtensionAdapter,
  probeExtensionAdapter,
  disconnectExtensionAdapter,
  type ExtensionAdapter,
  type ExtensionSnapshot,
  type ExtensionGuestReason
} from '../nostr/extension.ts';

declare const sessionBrand: unique symbol;
export type IdentitySession = Readonly<{ [sessionBrand]: true }>;
export type IdentitySnapshot = (
  | Readonly<{ state: 'guest'; reason: ExtensionGuestReason }>
  | Readonly<{ state: 'pending'; action: 'connect' | 'recheck' | 'probe' }>
  | Readonly<{
      state: 'connected' | 'signing_only' | 'messaging_capable';
      publicKey: string;
      messaging: 'not_probed' | 'unsupported' | 'refused' | 'capable';
    }>
) &
  Readonly<{ admission?: 'busy' }>;
// Per-client opaque session, never a module-global server user. A public key is
// an extension observation, not installed-account or publishing authorization.
const sessions = new WeakMap<IdentitySession, ExtensionAdapter>();
export function createIdentitySession(): IdentitySession {
  const session = Object.freeze({}) as IdentitySession;
  sessions.set(session, createExtensionAdapter());
  return session;
}
function mapIdentitySnapshot(snapshot: ExtensionSnapshot): IdentitySnapshot {
  if (snapshot.state !== 'connected') return { ...snapshot };
  const state: IdentitySnapshot = {
    state:
      snapshot.messaging === 'capable' && snapshot.signingCandidate
        ? 'messaging_capable'
        : snapshot.signingCandidate
          ? 'signing_only'
          : 'connected',
    publicKey: snapshot.publicKey,
    messaging: snapshot.messaging
  };
  return snapshot.admission === 'busy'
    ? { ...state, admission: 'busy' }
    : state;
}
export function identitySessionSnapshot(
  session: IdentitySession
): IdentitySnapshot {
  const adapter = sessions.get(session);
  return adapter
    ? mapIdentitySnapshot(extensionSnapshot(adapter))
    : { state: 'guest', reason: 'unavailable' };
}
export async function connectIdentity(
  session: IdentitySession
): Promise<IdentitySnapshot> {
  const adapter = sessions.get(session);
  if (adapter)
    return mapIdentitySnapshot(await connectExtensionAdapter(adapter));
  return identitySessionSnapshot(session);
}
export async function recheckIdentityOwner(
  session: IdentitySession
): Promise<IdentitySnapshot> {
  const adapter = sessions.get(session);
  if (adapter)
    return mapIdentitySnapshot(await recheckExtensionAdapter(adapter));
  return identitySessionSnapshot(session);
}
export async function probeIdentityMessaging(
  session: IdentitySession,
  review: unknown
): Promise<IdentitySnapshot> {
  const adapter = sessions.get(session);
  if (adapter)
    return mapIdentitySnapshot(await probeExtensionAdapter(adapter, review));
  return identitySessionSnapshot(session);
}
export function disconnectIdentity(session: IdentitySession): void {
  const adapter = sessions.get(session);
  if (adapter) disconnectExtensionAdapter(adapter);
}
