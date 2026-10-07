import type { EventTemplate } from 'applesauce-core/helpers/event';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import type { PublicEffectCapture } from '../runtime/effect-ownership.ts';

// Internal construction shape only, not a validated seal/recipient plan. Later
// preparation must establish the peer or self-copy target and authentic seal.
export type PrivateOuterTemplate = Readonly<{
  kind: 1059;
  tags: readonly [readonly ['p', string]];
  content: EventTemplate['content'];
  created_at: EventTemplate['created_at'];
}>;
declare const envelopeBrand: unique symbol;
export type PrivateEnvelopeCapability = Readonly<{
  [envelopeBrand]: PrivateOuterTemplate;
}>;
type ConstructionOwnership = Readonly<PublicEffectCapture & { kind: 1059 }>;
type Controller = {
  ownership(): ConstructionOwnership | undefined;
  close(): void;
};
const capabilities = new WeakMap<PrivateEnvelopeCapability, Controller>();
const sessions = new WeakMap<PrivateSession, PrivateEnvelopeCapability>();

// Executable initial permission boundary, deliberately unused by product sends
// until HCM015. No signer is created, passed in or exposed, and no identity key
// or raw key API exists. Later Applesauce disposable-key use stays wholly inside
// the outer construction adapter; this token cannot sign a listing or decrypt.
export function getPrivateEnvelopeCapability(
  session: PrivateSession
): PrivateEnvelopeCapability | undefined {
  if (typeof window === 'undefined') return undefined;
  const observed = privateSessionOwnership(session);
  if (!observed) throw new Error('private_session_invalid');
  const capture = observed;
  const prior = sessions.get(session);
  if (prior) {
    if (!controller(prior).ownership())
      throw new Error('private_envelope_capability_closed');
    return prior;
  }
  const token = Object.freeze({}) as PrivateEnvelopeCapability;
  let closed = false;
  let off = () => {};
  function close() {
    if (closed) return;
    closed = true;
    off();
    capabilities.set(token, {
      ownership: () => undefined,
      close: () => {}
    });
  }
  function current() {
    if (closed || !capture.current()) {
      close();
      return false;
    }
    return true;
  }
  capabilities.set(token, {
    ownership: () =>
      current()
        ? {
            kind: 1059,
            owner: capture.owner,
            session: capture.session,
            current
          }
        : undefined,
    close
  });
  off = subscribePrivateSessionClose(session, close);
  if (!current()) {
    close();
    return undefined;
  }
  sessions.set(session, token);
  return token;
}
function controller(token: PrivateEnvelopeCapability): Controller {
  const state = capabilities.get(token);
  if (!state) throw new Error('private_envelope_capability_invalid');
  return state;
}
export function privateEnvelopeConstructionOwnership(
  token: PrivateEnvelopeCapability
): ConstructionOwnership | undefined {
  return controller(token).ownership();
}
export function closePrivateEnvelopeCapability(
  token: PrivateEnvelopeCapability
): void {
  controller(token).close();
}
