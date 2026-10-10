import {
  identityMessagingOwnership,
  type IdentitySession
} from '../runtime/identity-session.ts';
import { privateSessionOwnership } from '../runtime/private-session.ts';
import {
  inboxSetupViewOwnership,
  inboxSetupViewSnapshot,
  type InboxSetupView
} from '../messaging/inbox-setup-view.ts';
import {
  inboxResolutionSnapshot,
  inboxResolverCurrentAfterSample
} from '../messaging/resolve-inbox.ts';
import { existingInboxSetupSnapshot } from '../messaging/inbox-setup-verification.ts';
import type { InboxAccessObservation } from '../messaging/readiness.ts';
import type { RelayPolicy } from '../config/relays.ts';

declare const readinessBrand: unique symbol;
export type NewListingReadiness = Readonly<{ [readinessBrand]: true }>;
type Ownership = Readonly<{
  owner: string;
  identity: IdentitySession;
  policy: RelayPolicy;
  current(this: void): boolean;
  signingCurrent(this: void): boolean;
}>;
const reviews = new WeakMap<
  NewListingReadiness,
  {
    ownership: Ownership;
    close(): void;
  }
>();

// Readiness is a prerequisite for a later new-listing command, not permission
// to sign, write a preference, reserve a listing or publish an event. Its owners
// must still validate the exact draft/author/template and final explicit action.
function sample(identity: IdentitySession, setup: InboxSetupView | undefined) {
  try {
    if (typeof window === 'undefined') return;
    const actor = identityMessagingOwnership(identity);
    const own = inboxSetupViewOwnership(setup);
    const state = inboxSetupViewSnapshot(setup);
    if (
      !actor?.current() ||
      !own?.current() ||
      own.identity !== identity ||
      !own.session ||
      !own.resolver ||
      state.busy ||
      !state.setupComplete
    )
      return;
    const privateOwner = privateSessionOwnership(own.session);
    if (
      !privateOwner?.current() ||
      privateOwner.owner !== actor.owner ||
      privateOwner.session !== actor.session
    )
      return;
    const initial = inboxResolutionSnapshot(own.resolver);
    if (!initial.knownBase || initial.author !== actor.owner) return;
    let access: InboxAccessObservation | undefined;
    const ready = existingInboxSetupSnapshot(
      identity,
      own.session,
      own.policy,
      own.resolver,
      own.observeAccess
        ? (context) => {
            access = own.observeAccess!(context);
            return access;
          }
        : undefined
    );
    const final = inboxResolutionSnapshot(own.resolver);
    const accessCurrent = access?.current() ?? false;
    const actual = inboxSetupViewOwnership(setup);
    const finalState = inboxSetupViewSnapshot(setup);
    if (
      !ready.setupComplete ||
      !ready.newListingReady ||
      !accessCurrent ||
      final.knownBase?.id !== initial.knownBase.id ||
      final.status !== 'ready' ||
      !actor.current() ||
      !privateOwner.current() ||
      !own.current() ||
      !actual?.current() ||
      actual.identity !== identity ||
      actual.policy !== own.policy ||
      actual.session !== own.session ||
      actual.resolver !== own.resolver ||
      finalState.busy ||
      !finalState.setupComplete ||
      !inboxResolverCurrentAfterSample(own.resolver)
    )
      return;
    return {
      actor,
      own,
      privateOwner,
      preferenceId: final.knownBase.id,
      accessCurrent: () => access?.current() ?? false
    };
  } catch {
    return;
  }
}

// Detached display information never mints or replaces the original review.
// Draft saving has no dependency on this module and remains available here.
export function newListingReadinessSnapshot(
  identity: IdentitySession,
  setup: InboxSetupView | undefined
) {
  const ready = !!sample(identity, setup);
  return {
    draft: true as const,
    readyToReview: ready,
    preferenceWrite: 'not_requested' as const,
    copy: ready
      ? 'Messages: ready to receive replies'
      : 'Enable messages before publishing so buyers can contact you.'
  };
}
export function captureNewListingReadiness(
  identity: IdentitySession,
  setup: InboxSetupView | undefined,
  review: unknown
): NewListingReadiness | undefined {
  if (review !== 'reviewed_new_listing_inbox') return;
  const captured = sample(identity, setup);
  if (!captured) return;
  const capturedResolver = captured.own.resolver;
  if (!capturedResolver) return;
  let active = true;
  // Continuity of an already admitted original signing action. Reacquiring
  // messaging capability during that action would reject its own busy prompt.
  // This checks retained original generations/access, never admits a new action.
  const signingCurrent = () => {
    const actual = inboxSetupViewOwnership(setup);
    const state = inboxSetupViewSnapshot(setup);
    const resolved = inboxResolutionSnapshot(capturedResolver);
    return (
      active &&
      captured.actor.current() &&
      captured.own.current() &&
      captured.privateOwner.current() &&
      captured.accessCurrent() &&
      !!actual?.current() &&
      actual.identity === captured.own.identity &&
      actual.policy === captured.own.policy &&
      actual.session === captured.own.session &&
      actual.resolver === captured.own.resolver &&
      !state.busy &&
      state.setupComplete &&
      resolved.status === 'ready' &&
      resolved.author === captured.actor.owner &&
      resolved.knownBase?.id === captured.preferenceId &&
      inboxResolverCurrentAfterSample(capturedResolver) &&
      active &&
      captured.actor.current() &&
      captured.privateOwner.current()
    );
  };
  const current = () => {
    if (
      !active ||
      !captured.actor.current() ||
      !captured.own.current() ||
      !captured.privateOwner.current()
    )
      return false;
    const actual = sample(identity, setup);
    return (
      !!actual &&
      active &&
      actual.actor.owner === captured.actor.owner &&
      actual.actor.session === captured.actor.session &&
      actual.own.identity === captured.own.identity &&
      actual.own.policy === captured.own.policy &&
      actual.own.session === captured.own.session &&
      actual.own.resolver === captured.own.resolver &&
      actual.preferenceId === captured.preferenceId &&
      captured.actor.current() &&
      captured.own.current() &&
      captured.privateOwner.current()
    );
  };
  const token = Object.freeze({}) as NewListingReadiness;
  reviews.set(token, {
    ownership: {
      owner: captured.actor.owner,
      identity,
      policy: captured.own.policy,
      current,
      signingCurrent
    },
    close: () => {
      active = false;
    }
  });
  return token;
}
export function newListingReadinessOwnership(
  token: NewListingReadiness | undefined
): Ownership | undefined {
  const original = token && reviews.get(token);
  return original?.ownership.current() ? { ...original.ownership } : undefined;
}
export function closeNewListingReadiness(token: NewListingReadiness): void {
  reviews.get(token)?.close();
}
