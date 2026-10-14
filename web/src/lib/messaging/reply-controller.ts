import { inboxReplyOwnership, type InboxView } from './inbox-view.ts';
import { inboxSetupViewOwnership } from './inbox-setup-view.ts';
import { messagingReadinessSnapshot } from './readiness.ts';
import { operationEnabled } from '../config/operation-flags.ts';
import {
  createInboxRoutePlan,
  inboxRoutePlanSnapshot
} from './inbox-routing.ts';
import {
  resolveInboxPreference,
  closeInboxResolver,
  type InboxResolver
} from './resolve-inbox.ts';
import {
  createPublicView,
  beginPublicViewRun,
  publicViewRunCurrent,
  disposePublicView,
  type PublicRuntime
} from '../runtime/public-runtime.ts';
import {
  createPrivateComposer,
  privateComposerText,
  privateComposerSnapshot,
  updatePrivateComposerText,
  bindPrivateComposerPreparation,
  closePrivateComposer
} from './composer-state.ts';
import {
  captureDirtyNavigation,
  requestPrivateNavigation,
  confirmDiscardNavigation,
  keepPrivateEditing,
  dirtyNavigationSnapshot,
  privateNavigationBeforeUnload
} from '../runtime/dirty-navigation.ts';
import {
  captureAuthenticatedReplyRumor,
  stopRumorPlan,
  type RumorPlan
} from './rumor-plan.ts';
import { createPrivateSendReservationRepository } from '../persistence/private-send-reservations.ts';
import { reserveSendIdentity } from './send-identity.ts';
import {
  captureSelfRecoveryPreparation,
  prepareSelfRecovery,
  preparedSelfRecovery,
  preparePairedDelivery,
  preparedPairedDelivery,
  selfRecoveryPreparationSnapshot,
  type SelfRecoveryPreparation
} from './prepare-send.ts';
import { getPrivatePool } from '../nostr/private-pool.ts';
import {
  capturePrivateRetry,
  runPrivateRetry,
  stopPrivateRetry,
  type PrivateRetry
} from './retry-send.ts';
import { newLocalId } from '../private-handles.ts';
import { subscribeIdentityInvalidation } from '../runtime/identity-session.ts';
import { subscribePrivateSessionClose } from '../runtime/private-session.ts';

declare const replyBrand: unique symbol;
export type ReplyController = Readonly<{ [replyBrand]: true }>;
type Result = Readonly<{
  status: 'invalid' | 'busy' | 'stopped' | 'needs_action' | 'completed';
}>;
type Options = Readonly<{
  lookupPeer?(peer: string): Promise<InboxResolver>;
  close?(): void;
}>;
type State =
  | 'idle'
  | 'checking'
  | 'preparing'
  | 'publishing'
  | 'needs_action'
  | 'completed';
type Snapshot = Readonly<{
  state: State;
  readyToSend: boolean;
  busy: boolean;
  dirty: boolean;
  copy: string;
  saveCopy: string;
  pairCopy: string;
  navigation: 'idle' | 'confirmation_required';
}>;
type Controller = {
  snapshot(): Snapshot | undefined;
  text(): string | undefined;
  update(text: unknown, review: unknown): boolean;
  select(parent: unknown, review: unknown): boolean;
  send(review: unknown): Promise<Result>;
  request(): ReturnType<typeof requestPrivateNavigation>;
  keep(): boolean;
  discard(review: unknown): ReturnType<typeof confirmDiscardNavigation>;
  unload(event: BeforeUnloadEvent): boolean;
  close(): void;
};
const controllers = new WeakMap<ReplyController, Controller>();

// Allocating or typing a draft performs no SDK, native write or discovery.
// Caller hints are resolved through the original selected view's Room/cache.
export function createReplyController(
  view: InboxView,
  conversationId: unknown,
  parent: unknown,
  options: Options = {}
): ReplyController | undefined {
  if (typeof window === 'undefined') return;
  let scope = inboxReplyOwnership(view, conversationId, parent);
  if (!scope?.current()) return;
  const original = scope,
    composer = createPrivateComposer(
      original.identity,
      'reviewed_private_composer'
    );
  const navigation = composer && captureDirtyNavigation(composer);
  if (!composer || !navigation) {
    if (composer) closePrivateComposer(composer);
    return;
  }
  let active = true,
    busy = false,
    state: State = 'idle',
    copy = '',
    attempted = false;
  let plan: RumorPlan | undefined,
    preparation: SelfRecoveryPreparation | undefined,
    retry: PrivateRetry | undefined;
  let identityOff = () => {},
    privateOff = () => {};
  function close() {
    if (!active) return;
    active = false;
    if (retry) stopPrivateRetry(retry);
    closePrivateComposer(composer!);
    if (plan) stopRumorPlan(plan);
    identityOff();
    privateOff();
    options.close?.();
  }
  function current() {
    if (!active || !scope?.current() || !privateComposerSnapshot(composer!)) {
      close();
      return false;
    }
    return true;
  }
  function snapshot(): Snapshot | undefined {
    if (!current()) return;
    const draft = privateComposerSnapshot(composer!),
      workflow = preparation && selfRecoveryPreparationSnapshot(preparation);
    if (!draft) return;
    return {
      state,
      readyToSend:
        !attempted &&
        operationEnabled(
          inboxSetupViewOwnership(scope!.setup)?.policy,
          'messaging'
        ),
      busy,
      dirty: draft.dirty,
      copy:
        copy ||
        (!operationEnabled(
          inboxSetupViewOwnership(scope!.setup)?.policy,
          'messaging'
        )
          ? 'Messaging is disabled in this build. Keep your unsent draft and review existing encrypted work separately.'
          : state === 'preparing'
            ? 'Preparing encryption. The extension may ask for several approvals.'
            : state === 'publishing'
              ? 'Publishing encrypted copies to the reviewed inbox relays.'
              : ''),
      saveCopy: draft.copy,
      pairCopy: workflow?.pairCopy ?? draft.pairCopy,
      navigation: dirtyNavigationSnapshot(navigation!)?.state ?? 'idle'
    };
  }
  async function send(review: unknown): Promise<Result> {
    if (review !== 'reviewed_reply_send') return { status: 'invalid' };
    if (!current()) return { status: 'stopped' };
    if (busy) return { status: 'busy' };
    if (
      !operationEnabled(
        inboxSetupViewOwnership(scope!.setup)?.policy,
        'messaging'
      )
    ) {
      state = 'needs_action';
      copy =
        'Messaging is disabled in this build. Your unsent text remains in memory; existing encrypted work needs its own recovery review.';
      return { status: 'needs_action' };
    }
    const text = privateComposerText(composer!);
    if (!text?.trim()) return { status: 'invalid' };
    if (attempted) return { status: 'needs_action' };
    busy = true;
    state = 'checking';
    copy =
      'Checking the authenticated peer inbox. Your text remains in memory.';
    const captured = scope!;
    const admitted = () => current() && scope === captured;
    function needsAction(definitePreEffect = false): Result {
      state = attempted || !definitePreEffect ? 'needs_action' : 'idle';
      if (!attempted) {
        if (plan) stopRumorPlan(plan);
        plan = undefined;
      }
      copy = attempted
        ? 'Send needs review. Keep this draft; check inbox setup or connection, then explicitly send again. Existing encrypted work needs its own recovery review.'
        : 'No private send was reserved. Your text stays in memory. Review inbox setup or storage, then explicitly try Send again.';
      return { status: 'needs_action' };
    }
    try {
      const setup = inboxSetupViewOwnership(captured.setup);
      if (!setup?.current() || !setup.resolver || !options.lookupPeer)
        return needsAction();
      const other = await options.lookupPeer(captured.peer);
      if (!admitted()) return { status: 'stopped' };
      if (
        !messagingReadinessSnapshot(
          captured.identity,
          captured.session,
          setup.policy,
          setup.resolver,
          { author: captured.peer, resolver: other },
          setup.observeAccess
        ).sendReady ||
        !admitted()
      )
        return needsAction();
      const routes = createInboxRoutePlan(
        setup.policy,
        captured.owner,
        captured.peer,
        setup.resolver,
        other
      );
      const routeData = routes && inboxRoutePlanSnapshot(routes);
      const reservations = createPrivateSendReservationRepository(
        captured.database,
        captured.owner
      );
      if (!routes || !routeData || !reservations || !admitted())
        return needsAction();
      plan = captureAuthenticatedReplyRumor(
        captured.identity,
        captured.session,
        captured.unlocked,
        captured.room,
        text,
        Math.floor(Date.now() / 1000)
      );
      const command = newLocalId();
      if (!plan || !command || !admitted()) return needsAction(true);
      // After a native intent may exist, repeated Send never creates another
      // rumor or silently replays this command. Recovery has its own owner.
      attempted = true;
      const reserved = await reserveSendIdentity(
        reservations,
        captured.identity,
        command,
        plan,
        'reviewed_private_intent'
      );
      if (!admitted()) return { status: 'stopped' };
      if (!('identity' in reserved)) {
        if (reserved.disposition !== 'not_created') return needsAction();
        attempted = false;
        return needsAction(true);
      }
      preparation = captureSelfRecoveryPreparation(
        captured.repository,
        captured.identity,
        reserved.identity,
        'reviewed_self_recovery'
      );
      if (
        !preparation ||
        !bindPrivateComposerPreparation(
          composer!,
          reserved.identity,
          preparation,
          undefined,
          'reviewed_composer_preparation'
        )
      )
        return needsAction();
      state = 'preparing';
      copy = '';
      const self = await prepareSelfRecovery(
        preparation,
        'reviewed_self_recovery'
      );
      if (!admitted()) return { status: 'stopped' };
      const saved = preparedSelfRecovery(preparation);
      if (
        !['saved', 'existing', 'reconciled'].includes(self.status) ||
        !saved ||
        !(await captured.acceptLocalReply(saved, 'reviewed_local_reply')) ||
        !admitted()
      )
        return needsAction();
      const context = {
        plan: routes,
        policy: setup.policy,
        own: setup.resolver,
        other
      };
      const paired = await preparePairedDelivery(
        preparation,
        context,
        'reviewed_pair_preparation'
      );
      if (!admitted()) return { status: 'stopped' };
      const receipt = preparedPairedDelivery(preparation);
      if (
        !['prepared', 'existing', 'reconciled'].includes(paired.status) ||
        !receipt
      )
        return needsAction();
      const origins = Array.from(
        new Set([...routeData.peer.targets, ...routeData.archive.targets])
      );
      const pool = getPrivatePool(captured.session, setup.policy, origins);
      if (!pool || !admitted()) return needsAction();
      retry = capturePrivateRetry(
        captured.repository,
        captured.session,
        receipt,
        context,
        pool,
        'reviewed_private_retry'
      );
      if (!retry || !admitted()) return needsAction();
      state = 'publishing';
      const delivered = await runPrivateRetry(retry, 'reviewed_private_retry');
      if (!admitted()) return { status: 'stopped' };
      if (delivered.status !== 'completed') return needsAction();
      state = 'completed';
      copy =
        'Accepted by recipient inbox relay. This does not confirm that the person read the message.';
      return { status: 'completed' };
    } catch {
      return current() ? needsAction() : { status: 'stopped' };
    } finally {
      busy = false;
    }
  }
  const token = Object.freeze({}) as ReplyController;
  controllers.set(token, {
    snapshot,
    text: () => (current() ? privateComposerText(composer) : undefined),
    update: (text, review) =>
      current() &&
      !busy &&
      !attempted &&
      updatePrivateComposerText(composer, text, review),
    select(parent, review) {
      if (review !== 'reviewed_reply_parent' || !current() || busy || attempted)
        return false;
      const selected = inboxReplyOwnership(view, conversationId, parent);
      if (
        !selected?.current() ||
        selected.identity !== original.identity ||
        selected.session !== original.session ||
        selected.peer !== original.peer
      )
        return false;
      scope = selected;
      return true;
    },
    send,
    request: () =>
      current() ? requestPrivateNavigation(navigation) : 'invalid',
    keep: () => current() && keepPrivateEditing(navigation),
    discard: (review) =>
      current() ? confirmDiscardNavigation(navigation, review) : 'invalid',
    unload: (event) =>
      current() && privateNavigationBeforeUnload(navigation, event),
    close
  });
  identityOff = subscribeIdentityInvalidation(original.identity, () => {
    if (!original.current()) close();
  });
  privateOff = subscribePrivateSessionClose(original.session, close);
  return current() ? token : undefined;
}

export function createRuntimeReplyController(
  view: InboxView,
  id: unknown,
  parent: unknown,
  runtime: PublicRuntime
): ReplyController | undefined {
  const publicView = createPublicView(runtime);
  let resolver: InboxResolver | undefined,
    run: ReturnType<typeof beginPublicViewRun> | undefined;
  const token = createReplyController(view, id, parent, {
    lookupPeer: (peer) => {
      // Only this explicit Send lookup may renew an expired run. An active
      // lookup retains its original bounded work, including pending results.
      if (!resolver || !run || !publicViewRunCurrent(publicView, run)) {
        if (resolver) closeInboxResolver(resolver);
        resolver = undefined;
        run = beginPublicViewRun(publicView);
        resolver = resolveInboxPreference(publicView, run, peer);
      }
      return Promise.resolve(resolver);
    },
    close: () => {
      if (resolver) closeInboxResolver(resolver);
      disposePublicView(publicView);
    }
  });
  if (!token) disposePublicView(publicView);
  return token;
}
export function replySnapshot(token: ReplyController) {
  return controllers.get(token)?.snapshot();
}
export function replyText(token: ReplyController) {
  return controllers.get(token)?.text();
}
export function updateReplyText(
  token: ReplyController,
  text: unknown,
  review: unknown
) {
  return controllers.get(token)?.update(text, review) ?? false;
}
export function selectReplyParent(
  token: ReplyController,
  parent: unknown,
  review: unknown
) {
  return controllers.get(token)?.select(parent, review) ?? false;
}
export function sendReply(
  token: ReplyController,
  review: unknown
): Promise<Result> {
  return (
    controllers.get(token)?.send(review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function requestReplyNavigation(token: ReplyController) {
  return controllers.get(token)?.request() ?? 'invalid';
}
export function keepReplyEditing(token: ReplyController) {
  return controllers.get(token)?.keep() ?? false;
}
export function discardReplyNavigation(
  token: ReplyController,
  review: unknown
) {
  return controllers.get(token)?.discard(review) ?? 'invalid';
}
export function replyBeforeUnload(
  token: ReplyController,
  event: BeforeUnloadEvent
) {
  return controllers.get(token)?.unload(event) ?? false;
}
export function closeReplyController(token: ReplyController) {
  controllers.get(token)?.close();
}
