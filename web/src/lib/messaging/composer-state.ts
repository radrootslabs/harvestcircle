import {
  identityMessagingOwnership,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { PRIVATE_TRANSPORT_BUDGETS } from '../config/budgets.ts';
import { buildEnquiryMessage, type EnquiryContext } from './enquiry-context.ts';
import {
  reservedSendSnapshot,
  reservedSendRumorWire,
  type ReservedSendIdentity
} from './send-identity.ts';
import {
  selfRecoveryPreparationSnapshot,
  preparedSelfRecovery,
  stopSelfRecoveryPreparation,
  type SelfRecoveryPreparation
} from './prepare-send.ts';
import {
  selfRecoveryAcknowledgementSnapshot,
  type SelfRecoveryAcknowledgement
} from '../persistence/private-sends.ts';

declare const composerBrand: unique symbol;
export type PrivateComposer = Readonly<{ [composerBrand]: true }>;
type TextState =
  | 'empty'
  | 'unsaved'
  | 'preparing'
  | 'saved_encrypted'
  | 'unknown_completion'
  | 'needs_action';
type Snapshot = Readonly<{
  revision: number;
  dirty: boolean;
  busy: boolean;
  state: TextState;
  pairState:
    | 'unprepared'
    | 'preparing'
    | 'prepared'
    | 'unknown_completion'
    | 'needs_action';
  copy: string;
  pairCopy: string;
}>;
type Controller = {
  text(): string | undefined;
  update(text: unknown, review: unknown): boolean;
  snapshot(): Snapshot | undefined;
  bind(
    reserved: ReservedSendIdentity,
    preparation: SelfRecoveryPreparation,
    enquiry: EnquiryContext | undefined,
    review: unknown
  ): boolean;
  discard(revision: unknown, review: unknown): boolean;
  close(): void;
};
const composers = new WeakMap<PrivateComposer, Controller>();

// Private input is memory only. The controller has no storage, URL, logging,
// public-draft, signer or transport sink. Canonical message admission and all
// effects remain owned by the existing genuine intent/preparation workflow.
export function createPrivateComposer(
  session: IdentitySession,
  review: unknown
): PrivateComposer | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_private_composer')
    return undefined;
  const ownership = identityMessagingOwnership(session);
  if (!ownership?.current()) return undefined;
  let active = true,
    body = '',
    revision = 0,
    unsubscribe = () => {};
  type Binding = {
    revision: number;
    preparation: SelfRecoveryPreparation;
    owner: string;
    command: string;
    peer: string;
    rumorHash: string;
    createdAt: number;
  };
  let binding: Binding | undefined;
  let lastState: ReturnType<typeof selfRecoveryPreparationSnapshot>;
  let acknowledgement: SelfRecoveryAcknowledgement | undefined;
  function close() {
    if (!active) return;
    active = false;
    body = '';
    if (binding) stopSelfRecoveryPreparation(binding.preparation);
    binding = undefined;
    lastState = undefined;
    acknowledgement = undefined;
    unsubscribe();
    unsubscribe = () => {};
  }
  function current() {
    if (!active || !ownership?.current()) {
      close();
      return false;
    }
    return true;
  }
  function observe() {
    if (!binding) return undefined;
    const workflow = selfRecoveryPreparationSnapshot(binding.preparation);
    if (
      workflow?.owner !== binding.owner ||
      workflow.command !== binding.command
    )
      return undefined;
    lastState = workflow;
    return workflow;
  }
  function acknowledged() {
    if (!binding) return false;
    const workflow = selfRecoveryPreparationSnapshot(binding.preparation);
    const actual = preparedSelfRecovery(binding.preparation);
    // A genuine historical local fact may survive crypto retirement. A live
    // controller explicitly losing its acknowledgement must not hide that loss.
    if (workflow && !actual) acknowledgement = undefined;
    const receipt = actual ?? acknowledgement,
      saved = receipt && selfRecoveryAcknowledgementSnapshot(receipt);
    const matches =
      !!saved &&
      saved.owner === binding.owner &&
      saved.id === binding.command &&
      saved.peer === binding.peer &&
      saved.rumorHash === binding.rumorHash &&
      saved.createdAt === binding.createdAt;
    if (matches) acknowledgement = receipt;
    return matches;
  }
  function snapshot(): Snapshot | undefined {
    if (!current()) return undefined;
    const currentWorkflow = observe(),
      workflow =
        currentWorkflow ??
        (lastState
          ? {
              ...lastState,
              state:
                lastState.state === 'unknown_completion'
                  ? 'unknown_completion'
                  : 'needs_action',
              busy: false
            }
          : undefined),
      exact = binding?.revision === revision,
      saved = exact && acknowledged(),
      dirty = body.length > 0 && !saved;
    const state: TextState =
      body.length === 0
        ? 'empty'
        : saved
          ? 'saved_encrypted'
          : !exact
            ? 'unsaved'
            : workflow?.state === 'unknown_completion'
              ? 'unknown_completion'
              : workflow?.state === 'preparing'
                ? 'preparing'
                : workflow?.state === 'needs_action' || !workflow
                  ? 'needs_action'
                  : 'unsaved';
    return {
      revision,
      dirty,
      busy: currentWorkflow?.busy ?? false,
      state,
      pairState: workflow?.pairState ?? 'unprepared',
      copy:
        state === 'saved_encrypted'
          ? 'Saved encrypted in this browser'
          : state === 'unknown_completion'
            ? 'Encrypted save could not be confirmed. Keep this text and review the existing preparation.'
            : state === 'preparing'
              ? 'Private text is unsaved memory while encryption is preparing.'
              : state === 'needs_action'
                ? 'Private text has no acknowledged encrypted save in this browser. Keep editing or review preparation.'
                : state === 'empty'
                  ? 'Private text stays in memory until encrypted capture.'
                  : 'Private text is unsaved memory.',
      pairCopy:
        workflow?.pairCopy ?? 'No paired delivery preparation is acknowledged.'
    };
  }
  function update(text: unknown, reviewed: unknown) {
    if (
      reviewed !== 'reviewed_private_text' ||
      !current() ||
      typeof text !== 'string' ||
      !boundedUtf8(text, PRIVATE_TRANSPORT_BUDGETS.bodyBytes)
    )
      return false;
    if (body !== text) {
      body = text;
      revision++;
    }
    return true;
  }
  function bind(
    reserved: ReservedSendIdentity,
    preparation: SelfRecoveryPreparation,
    enquiry: EnquiryContext | undefined,
    reviewed: unknown
  ) {
    if (reviewed !== 'reviewed_composer_preparation' || !current())
      return false;
    const previous = observe();
    if (
      (binding && !previous) ||
      previous?.busy ||
      previous?.state === 'unknown_completion' ||
      previous?.pairState === 'unknown_completion'
    )
      return false;
    const identity = reservedSendSnapshot(reserved),
      wire = reservedSendRumorWire(reserved),
      workflow = selfRecoveryPreparationSnapshot(preparation);
    if (
      !identity ||
      !wire ||
      !workflow ||
      identity.owner !== ownership?.owner ||
      workflow.owner !== identity.owner ||
      workflow.command !== identity.id
    )
      return false;
    // This wire is available only from the genuine current reserved identity;
    // no caller-provided body/hash can substitute for its canonical rumor.
    let content: unknown;
    try {
      content = (JSON.parse(wire) as { content: unknown }).content;
    } catch {
      return false;
    }
    const expected =
      enquiry === undefined
        ? body
        : buildEnquiryMessage(enquiry, body)?.content;
    if (typeof expected !== 'string' || content !== expected) return false;
    if (
      binding &&
      (binding.command !== identity.id || binding.preparation !== preparation)
    )
      stopSelfRecoveryPreparation(binding.preparation);
    binding = {
      revision,
      preparation,
      owner: identity.owner,
      command: identity.id,
      peer: identity.peer,
      rumorHash: identity.rumorHash,
      createdAt: identity.createdAt
    };
    lastState = undefined;
    acknowledgement = undefined;
    return true;
  }
  function discard(expectedRevision: unknown, reviewed: unknown) {
    if (
      reviewed !== 'reviewed_discard_private_text' ||
      !current() ||
      expectedRevision !== revision
    )
      return false;
    // Retirement revokes continuations, but the shared SDK scheduler retains
    // an unsettled extension job until the actual promise completes.
    if (binding) stopSelfRecoveryPreparation(binding.preparation);
    binding = undefined;
    lastState = undefined;
    acknowledgement = undefined;
    body = '';
    revision++;
    return true;
  }
  const token = Object.freeze({}) as PrivateComposer;
  composers.set(token, {
    text: () => (current() ? body : undefined),
    update,
    snapshot,
    bind,
    discard,
    close
  });
  unsubscribe = subscribeIdentityInvalidation(session, () => {
    if (!ownership.current()) close();
  });
  return current() ? token : undefined;
}
export function updatePrivateComposerText(
  token: PrivateComposer,
  text: unknown,
  review: unknown
) {
  return composers.get(token)?.update(text, review) ?? false;
}
export function privateComposerText(token: PrivateComposer) {
  return composers.get(token)?.text();
}
export function privateComposerSnapshot(token: PrivateComposer) {
  return composers.get(token)?.snapshot();
}
export function bindPrivateComposerPreparation(
  token: PrivateComposer,
  reserved: ReservedSendIdentity,
  preparation: SelfRecoveryPreparation,
  enquiry: EnquiryContext | undefined,
  review: unknown
) {
  return (
    composers.get(token)?.bind(reserved, preparation, enquiry, review) ?? false
  );
}
export function discardPrivateComposer(
  token: PrivateComposer,
  revision: unknown,
  review: unknown
) {
  return composers.get(token)?.discard(revision, review) ?? false;
}
export function closePrivateComposer(token: PrivateComposer) {
  composers.get(token)?.close();
  composers.delete(token);
}
