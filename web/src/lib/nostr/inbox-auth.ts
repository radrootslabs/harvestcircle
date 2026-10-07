import { ExtensionSigner } from 'applesauce-signers';
import { getEventHash, type UnsignedEvent } from 'applesauce-core/helpers';
import { verifyEnvelope } from './verified-envelope.ts';
import {
  beginPrivateAuthConnection,
  privateAuthChallenge,
  privateAuthStopped,
  reservePrivateAuthResponse,
  sendPrivateAuthResponse,
  closePrivateAuthConnection,
  invalidatePrivateAuthGeneration,
  type PrivatePool,
  type PrivateAuthChallenge
} from './private-pool.ts';
import {
  browserExtensionScheduler,
  runExtensionAction,
  callExtension
} from './extension-scheduler.ts';
declare const authBrand: unique symbol;
export type InboxAuthAction = Readonly<{ [authBrand]: true }>;
type State = {
  snapshot(): { state: string; responses: number };
  run(): Promise<Readonly<{ status: string }>>;
  close(): void;
};
const actions = new WeakMap<InboxAuthAction, State>();
declare const admissionBrand: unique symbol;
declare const responseBrand: unique symbol;
export type InboxAuthAdmission = Readonly<{ [admissionBrand]: true }>;
export type GuardedInboxAuthResponse = Readonly<{ [responseBrand]: true }>;
const admissions = new WeakMap<
  InboxAuthAdmission,
  Readonly<{ pool: PrivatePool; origin: string }>
>();
const signedResponses = new WeakMap<
  GuardedInboxAuthResponse,
  Readonly<{ proof: PrivateAuthChallenge; wire: string; current(): boolean }>
>();
// Read/consume ports cannot manufacture approval. Only the lexical reviewed
// action and actual scheduler continuation create these genuine capabilities.
export function takeInboxAuthAdmission(
  token: InboxAuthAdmission,
  pool: PrivatePool,
  origin: string
): boolean {
  const saved = admissions.get(token);
  if (!saved || saved.pool !== pool || saved.origin !== origin) return false;
  admissions.delete(token);
  return true;
}
export function takeGuardedInboxAuthResponse(
  token: GuardedInboxAuthResponse,
  proof: PrivateAuthChallenge
): string | undefined {
  const saved = signedResponses.get(token);
  if (!saved || saved.proof !== proof || !saved.current() || !proof.current())
    return undefined;
  signedResponses.delete(token);
  return saved.wire;
}
export async function beginInboxAuthentication(
  pool: PrivatePool,
  origin: string,
  review: unknown
): Promise<InboxAuthAction | undefined> {
  if (typeof window === 'undefined' || review !== 'reviewed_connection_auth')
    return undefined;
  const admission = Object.freeze({}) as InboxAuthAdmission;
  admissions.set(admission, { pool, origin });
  const acquired = await beginPrivateAuthConnection(pool, origin, admission);
  if (!acquired) return undefined;
  const connection = acquired;
  const token = Object.freeze({}) as InboxAuthAction;
  let busy = false,
    responses = 0;
  let phase: 'waiting' | 'accepted' | 'refused' | 'unknown' | 'stopped' =
    'waiting';
  function close() {
    phase = 'stopped';
    closePrivateAuthConnection(connection);
  }
  function own(value: object, name: string): unknown {
    const field = Object.getOwnPropertyDescriptor(value, name);
    return field && 'value' in field ? field.value : undefined;
  }
  function messagingCapabilitiesCurrent(): boolean {
    try {
      const cipher = new ExtensionSigner().nip44;
      return (
        typeof cipher?.encrypt === 'function' &&
        typeof cipher?.decrypt === 'function'
      );
    } catch {
      return false;
    }
  }
  // Fixed seven-field response reconstruction never invokes provider getters,
  // iterators/toJSON or mutable cached SDK signature markers.
  function bindResponse(
    template: UnsignedEvent,
    response: unknown
  ): string | undefined {
    try {
      if (!response || typeof response !== 'object' || Array.isArray(response))
        return undefined;
      for (const name of ['pubkey', 'kind', 'created_at', 'content'] as const)
        if (own(response, name) !== template[name]) return undefined;
      const tags = own(response, 'tags');
      if (!Array.isArray(tags) || own(tags, 'length') !== 2) return undefined;
      for (let i = 0; i < 2; i++) {
        const tag = own(tags, String(i));
        if (
          !Array.isArray(tag) ||
          own(tag, 'length') !== 2 ||
          own(tag, '0') !== template.tags[i][0] ||
          own(tag, '1') !== template.tags[i][1]
        )
          return undefined;
      }
      const id = own(response, 'id'),
        sig = own(response, 'sig');
      if (
        typeof id !== 'string' ||
        typeof sig !== 'string' ||
        id !== getEventHash(template)
      )
        return undefined;
      const wire = JSON.stringify({ ...template, id, sig });
      return verifyEnvelope(wire).ok ? wire : undefined;
    } catch {
      return undefined;
    }
  }
  async function run(): Promise<Readonly<{ status: string }>> {
    if (busy) return { status: 'busy' };
    const proof = privateAuthChallenge(connection),
      scheduler = browserExtensionScheduler();
    if (
      !proof ||
      !scheduler ||
      ['refused', 'unknown', 'stopped'].includes(phase)
    )
      return { status: 'stopped' };
    busy = true;
    try {
      const result = await runExtensionAction(
        scheduler,
        { owner: proof.owner, session: proof.session, operation: Symbol() },
        () => proof.current() && phase !== 'stopped',
        async (job) => {
          if (!reservePrivateAuthResponse(connection, proof))
            return { status: 'stopped' };
          responses++;
          const signer = new ExtensionSigner();
          const key = await callExtension(job, 'key', () =>
            new ExtensionSigner().getPublicKey()
          );
          if (
            proof.current() &&
            (key.status === 'denied' ||
              (key.status === 'settled' &&
                key.current &&
                (key.value !== proof.owner || !messagingCapabilitiesCurrent())))
          )
            invalidatePrivateAuthGeneration(connection, proof);
          if (
            key.status !== 'settled' ||
            !key.current ||
            key.value !== proof.owner ||
            !proof.current()
          ) {
            close();
            return { status: 'stopped' };
          }
          const time = Math.floor(Date.now() / 1000);
          if (!Number.isSafeInteger(time) || time < 0 || !proof.current()) {
            close();
            return { status: 'stopped' };
          }
          const template: UnsignedEvent = {
            pubkey: proof.owner,
            kind: 22242,
            created_at: time,
            tags: [
              ['relay', proof.relay],
              ['challenge', proof.challenge]
            ],
            content: ''
          };
          const response = await callExtension(job, 'auth', () =>
            signer.signEvent(
              JSON.parse(JSON.stringify(template)) as UnsignedEvent
            )
          );
          if (
            response.status !== 'settled' ||
            !response.current ||
            !proof.current()
          ) {
            close();
            return {
              status: response.status === 'denied' ? 'denied' : 'stopped'
            };
          }
          const wire = bindResponse(template, response.value);
          if (!wire || !proof.current()) {
            close();
            return { status: 'invalid_response' };
          }
          // The provider can change its selected identity during signing. Fresh
          // actual SDK key evidence is required again before the AUTH effect.
          const effectKey = await callExtension(job, 'key', () =>
            new ExtensionSigner().getPublicKey()
          );
          if (
            proof.current() &&
            (effectKey.status === 'denied' ||
              (effectKey.status === 'settled' &&
                effectKey.current &&
                (effectKey.value !== proof.owner ||
                  !messagingCapabilitiesCurrent())))
          )
            invalidatePrivateAuthGeneration(connection, proof);
          if (
            effectKey.status !== 'settled' ||
            !effectKey.current ||
            effectKey.value !== proof.owner ||
            !proof.current()
          ) {
            close();
            return { status: 'stopped' };
          }
          const approved = Object.freeze({}) as GuardedInboxAuthResponse;
          signedResponses.set(approved, {
            proof,
            wire,
            current: () => proof.current() && phase !== 'stopped'
          });
          const status = await sendPrivateAuthResponse(
            connection,
            proof,
            approved
          );
          phase =
            status === 'accepted'
              ? 'accepted'
              : status === 'refused'
                ? 'refused'
                : status === 'unknown'
                  ? 'unknown'
                  : 'stopped';
          return { status };
        }
      );
      return 'value' in result &&
        result.value &&
        (result.status === 'completed' ||
          result.value.status === 'refused' ||
          result.value.status === 'unknown')
        ? result.value
        : { status: result.status };
    } finally {
      busy = false;
    }
  }
  actions.set(token, {
    run,
    close,
    snapshot: () => ({
      state:
        privateAuthStopped(connection) &&
        phase !== 'refused' &&
        phase !== 'unknown'
          ? 'stopped'
          : busy
            ? 'signing'
            : privateAuthChallenge(connection)
              ? 'challenge'
              : phase,
      responses
    })
  });
  return token;
}
export function inboxAuthSnapshot(action: InboxAuthAction) {
  return actions.get(action)?.snapshot();
}
export function closeInboxAuthentication(action: InboxAuthAction): void {
  actions.get(action)?.close();
}
export function respondInboxAuthentication(
  action: InboxAuthAction
): Promise<Readonly<{ status: string }>> {
  return actions.get(action)?.run() ?? Promise.resolve({ status: 'invalid' });
}
