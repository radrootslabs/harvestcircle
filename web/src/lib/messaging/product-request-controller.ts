import {
  productEnquirySnapshot,
  productEnquirySendOwnership,
  updateProductEnquiry,
  closeProductEnquiry,
  type ProductEnquiry
} from './product-enquiry.ts';
import {
  identityMessagingOwnership,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  inboxSetupViewOwnership,
  type InboxSetupView
} from './inbox-setup-view.ts';
import { messagingReadinessSnapshot } from './readiness.ts';
import { operationEnabled } from '../config/operation-flags.ts';
import {
  sellerInboxStatus,
  sellerInboxCopy,
  type SellerInboxStatus
} from './enquiry-readiness.ts';
import {
  createInboxRoutePlan,
  inboxRoutePlanSnapshot
} from './inbox-routing.ts';
import {
  resolveInboxPreference,
  settleInboxResolver,
  closeInboxResolver,
  type InboxResolver
} from './resolve-inbox.ts';
import {
  createPublicView,
  beginPublicViewRun,
  disposePublicView,
  type PublicRuntime
} from '../runtime/public-runtime.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  type BrowserDatabase
} from '../persistence/database.ts';
import { createPrivateStorageRepository } from '../persistence/private-storage.ts';
import { createPrivateSendReservationRepository } from '../persistence/private-send-reservations.ts';
import {
  createPrivateComposer,
  updatePrivateComposerText,
  privateComposerSnapshot,
  bindPrivateComposerPreparation,
  closePrivateComposer,
  type PrivateComposer
} from './composer-state.ts';
import {
  captureEnquiryRumor,
  stopRumorPlan,
  type RumorPlan
} from './rumor-plan.ts';
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
import { privateSessionOwnership } from '../runtime/private-session.ts';

declare const requestBrand: unique symbol;
export type ProductRequestController = Readonly<{ [requestBrand]: true }>;
type Result = Readonly<{
  status: 'invalid' | 'busy' | 'stopped' | 'needs_action' | 'completed';
}>;
type NavigationResult =
  'invalid' | 'allowed' | 'confirmation_required' | 'review_required';
type State =
  | 'idle'
  | 'checking'
  | 'preparing'
  | 'publishing'
  | 'needs_action'
  | 'completed';
type Snapshot = Readonly<{
  state: State;
  busy: boolean;
  dirty: boolean;
  revision: number;
  readyToSend: boolean;
  sellerInbox: SellerInboxStatus;
  copy: string;
  saveCopy: string;
  pairCopy: string;
  navigation: 'idle' | 'confirmation_required';
}>;
type Controller = {
  snapshot(): Snapshot | undefined;
  text(): string | undefined;
  update(text: unknown, review: unknown): boolean;
  configure(
    identity: IdentitySession,
    setup: InboxSetupView,
    review: unknown
  ): boolean;
  send(review: unknown): Promise<Result>;
  request(): NavigationResult;
  keep(): boolean;
  discard(review: unknown): NavigationResult;
  unload(event: BeforeUnloadEvent): boolean;
  close(): void;
  stop(review: unknown): boolean;
};
const controllers = new WeakMap<ProductRequestController, Controller>();

// Guest allocation and configuration preserve the original enquiry memory.
// Only the reviewed Send command acquires storage, crypto or publication.
export function createProductRequestController(
  enquiry: ProductEnquiry,
  runtime?: PublicRuntime
): ProductRequestController | undefined {
  if (typeof window === 'undefined' || !productEnquirySnapshot(enquiry)) return;
  let active = true,
    busy = false,
    attempted = false;
  let state: State = 'idle',
    copy = '',
    requestedRevision: number | undefined,
    sentRevision: number | undefined;
  let identity: IdentitySession | undefined, setup: InboxSetupView | undefined;
  let composer: PrivateComposer | undefined,
    database: BrowserDatabase | undefined;
  let plan: RumorPlan | undefined,
    preparation: SelfRecoveryPreparation | undefined,
    retry: PrivateRetry | undefined;
  let identityOff = () => {},
    generation = 0;
  const source = runtime && createPublicView(runtime);
  let resolver: InboxResolver | undefined;
  function stopWork(preserveComposer = false) {
    generation++;
    if (preserveComposer && composer) privateComposerSnapshot(composer);
    if (retry) stopPrivateRetry(retry);
    if (plan) stopRumorPlan(plan);
    if (!preserveComposer) {
      if (composer) closePrivateComposer(composer);
      composer = undefined;
    }
  }
  function close() {
    if (!active) return;
    active = false;
    stopWork();
    identityOff();
    closeProductEnquiry(enquiry);
    if (resolver) closeInboxResolver(resolver);
    if (source) disposePublicView(source);
    if (database) closeBrowserDatabase(database);
  }
  function current() {
    if (!active || !productEnquirySnapshot(enquiry)) {
      close();
      return false;
    }
    return true;
  }
  function ownership() {
    const actor = identity && identityMessagingOwnership(identity);
    const own = inboxSetupViewOwnership(setup);
    const product = productEnquirySendOwnership(enquiry);
    const data = productEnquirySnapshot(enquiry);
    return current() &&
      actor?.current() &&
      own?.current() &&
      own.identity === identity &&
      own.session &&
      product?.current() &&
      data &&
      data.peer !== actor.owner
      ? { actor, own, product, data }
      : undefined;
  }
  function ready() {
    const scope = ownership();
    return (
      !!scope &&
      !!source &&
      operationEnabled(scope.own.policy, 'messaging') &&
      messagingReadinessSnapshot(
        identity!,
        scope.own.session,
        scope.own.policy,
        scope.own.resolver,
        undefined,
        scope.own.observeAccess
      ).newListingReady
    );
  }
  function snapshot(): Snapshot | undefined {
    if (!current()) return;
    const actual = productEnquirySnapshot(enquiry);
    const draft =
      actual?.revision === sentRevision &&
      composer &&
      privateComposerSnapshot(composer);
    const workflow =
      preparation && selfRecoveryPreparationSnapshot(preparation);
    return {
      state,
      busy,
      dirty: draft
        ? draft.dirty
        : !!productEnquirySnapshot(enquiry)?.text.trim(),
      revision: actual?.revision ?? 0,
      readyToSend: !attempted && state !== 'needs_action' && ready(),
      sellerInbox: sellerInboxStatus(
        resolver,
        actual!.peer,
        inboxSetupViewOwnership(setup)?.policy
      ),
      copy:
        copy ||
        (inboxSetupViewOwnership(setup)?.policy &&
        !operationEnabled(inboxSetupViewOwnership(setup)?.policy, 'messaging')
          ? 'Messaging is disabled in this build. Keep your unsent draft and review existing encrypted work separately.'
          : state === 'preparing'
            ? 'Preparing encryption. The extension may ask for several approvals.'
            : state === 'publishing'
              ? 'Publishing encrypted copies to the reviewed inbox relays.'
              : ''),
      saveCopy: draft
        ? draft.copy
        : attempted
          ? 'Encrypted work needs review in this browser.'
          : 'Not yet saved.',
      pairCopy: workflow?.pairCopy ?? '',
      navigation:
        requestedRevision === undefined ? 'idle' : 'confirmation_required'
    };
  }
  async function send(review: unknown): Promise<Result> {
    if (review !== 'reviewed_product_request_send')
      return { status: 'invalid' };
    if (!current()) return { status: 'stopped' };
    if (busy) return { status: 'busy' };
    if (attempted) return { status: 'needs_action' };
    const requestedPolicy = inboxSetupViewOwnership(setup)?.policy;
    if (requestedPolicy && !operationEnabled(requestedPolicy, 'messaging')) {
      state = 'needs_action';
      copy =
        'Messaging is disabled in this build. Your unsent text remains in memory; existing encrypted work needs its own recovery review.';
      return { status: 'needs_action' };
    }
    const captured = ownership();
    if (
      !captured ||
      !ready() ||
      !identity ||
      !source ||
      !captured.data.text.trim()
    )
      return { status: 'invalid' };
    const actor = identity,
      originalSetup = setup,
      originalGeneration = generation;
    const privateOwner = privateSessionOwnership(captured.own.session!);
    if (
      !privateOwner?.current() ||
      privateOwner.owner !== captured.actor.owner ||
      privateOwner.session !== captured.actor.session
    )
      return { status: 'invalid' };
    const admitted = () =>
      current() &&
      generation === originalGeneration &&
      identity === actor &&
      setup === originalSetup &&
      captured.actor.current() &&
      captured.own.current() &&
      captured.product.current() &&
      privateOwner.current() &&
      productEnquirySnapshot(enquiry)?.revision === captured.data.revision &&
      inboxSetupViewOwnership(originalSetup)?.session === captured.own.session;
    function needsAction(definitePreEffect = false): Result {
      state = attempted || !definitePreEffect ? 'needs_action' : 'idle';
      if (!attempted) {
        sentRevision = undefined;
        if (plan) stopRumorPlan(plan);
        plan = undefined;
        if (composer) closePrivateComposer(composer);
        composer = undefined;
        if (database) closeBrowserDatabase(database);
        database = undefined;
      }
      copy = attempted
        ? 'Send needs review. Your unsent text stays in memory. Existing encrypted work requires its own recovery review.'
        : 'No private send was reserved. Your text stays in memory. Review inbox setup or storage, then explicitly try Send again.';
      return { status: 'needs_action' };
    }
    busy = true;
    state = 'checking';
    copy =
      'Checking the intended publisher inbox. Your text remains in memory.';
    try {
      if (resolver) closeInboxResolver(resolver);
      resolver = resolveInboxPreference(
        source,
        beginPublicViewRun(source),
        captured.data.peer
      );
      const other = resolver;
      // Resolver observations are asynchronous; sample their original bounded
      // result through the existing readiness projection, without guessing.
      await settleInboxResolver(other, admitted);
      if (!admitted()) {
        const unavailable = sellerInboxCopy(
          sellerInboxStatus(other, captured.data.peer, captured.own.policy)
        );
        if (
          unavailable &&
          current() &&
          generation === originalGeneration &&
          identity === actor &&
          setup === originalSetup &&
          captured.actor.current() &&
          captured.own.current() &&
          privateOwner.current() &&
          productEnquirySnapshot(enquiry)?.revision === captured.data.revision
        ) {
          state = 'needs_action';
          copy =
            (captured.product.current()
              ? ''
              : 'Listing status changed or could not be confirmed. Review the current listing before sending. ') +
            unavailable +
            ' Your unsent text stays in memory.';
        }
        return { status: 'stopped' };
      }
      if (
        !messagingReadinessSnapshot(
          actor,
          captured.own.session,
          captured.own.policy,
          captured.own.resolver,
          { author: captured.data.peer, resolver: other },
          captured.own.observeAccess
        ).sendReady
      ) {
        const result = needsAction();
        const unavailable = sellerInboxCopy(
          sellerInboxStatus(other, captured.data.peer, captured.own.policy)
        );
        if (unavailable)
          copy =
            unavailable +
            ' Your unsent text stays in memory. Optional other contact details may be available on the listing.';
        return result;
      }
      const routes = createInboxRoutePlan(
        captured.own.policy,
        captured.actor.owner,
        captured.data.peer,
        captured.own.resolver!,
        other
      );
      const routeData = routes && inboxRoutePlanSnapshot(routes);
      if (!routes || !routeData || !admitted()) return needsAction();
      const opened = await openBrowserDatabase();
      if (opened.state !== 'ready') return needsAction(true);
      database = opened.owner;
      if (!admitted()) return { status: 'stopped' };
      const repository = createPrivateStorageRepository(
        database,
        captured.actor.owner
      );
      const reservations = createPrivateSendReservationRepository(
        database,
        captured.actor.owner
      );
      composer = createPrivateComposer(actor, 'reviewed_private_composer');
      sentRevision = captured.data.revision;
      if (
        !repository ||
        !reservations ||
        !composer ||
        !updatePrivateComposerText(
          composer,
          captured.data.text,
          'reviewed_private_text'
        )
      )
        return needsAction(true);
      plan = captureEnquiryRumor(
        actor,
        captured.product.context,
        captured.data.text,
        Math.floor(Date.now() / 1000),
        admitted
      );
      const command = newLocalId();
      if (!plan || !command || !admitted()) return needsAction(true);
      attempted = true;
      const reserved = await reserveSendIdentity(
        reservations,
        actor,
        command,
        plan,
        'reviewed_private_intent'
      );
      if (!('identity' in reserved)) {
        // Stop can revoke this run while its original draft remains mounted.
        // A confirmed no-row result releases only that draft's latch; stale
        // key/account/body observations cannot restore or retarget another one.
        if (
          reserved.disposition === 'not_created' &&
          current() &&
          identity === actor &&
          setup === originalSetup &&
          captured.actor.current() &&
          privateOwner.current() &&
          productEnquirySnapshot(enquiry)?.revision === captured.data.revision
        ) {
          attempted = false;
          return needsAction(true);
        }
        return admitted() ? needsAction() : { status: 'stopped' };
      }
      if (!admitted()) return { status: 'stopped' };
      preparation = captureSelfRecoveryPreparation(
        repository,
        actor,
        reserved.identity,
        'reviewed_self_recovery'
      );
      if (
        !preparation ||
        !bindPrivateComposerPreparation(
          composer,
          reserved.identity,
          preparation,
          captured.product.context,
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
      if (
        !['saved', 'existing', 'reconciled'].includes(self.status) ||
        !preparedSelfRecovery(preparation)
      )
        return needsAction();
      const context = {
        plan: routes,
        policy: captured.own.policy,
        own: captured.own.resolver!,
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
      const pool = getPrivatePool(
        captured.own.session!,
        captured.own.policy,
        Array.from(
          new Set([...routeData.peer.targets, ...routeData.archive.targets])
        )
      );
      if (!pool || !admitted()) return needsAction();
      retry = capturePrivateRetry(
        repository,
        captured.own.session!,
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
      return admitted() ? needsAction() : { status: 'stopped' };
    } finally {
      busy = false;
    }
  }
  const token = Object.freeze({}) as ProductRequestController;
  controllers.set(token, {
    snapshot,
    text: () => (current() ? productEnquirySnapshot(enquiry)?.text : undefined),
    update(text, review) {
      if (
        review !== 'reviewed_private_text' ||
        !current() ||
        busy ||
        attempted ||
        !updateProductEnquiry(enquiry, text, 'reviewed_product_enquiry_edit')
      )
        return false;
      return true;
    },
    configure(nextIdentity, nextSetup, review) {
      if (
        review !== 'reviewed_product_request_setup' ||
        !current() ||
        attempted ||
        busy
      )
        return false;
      const actor = identityMessagingOwnership(nextIdentity),
        own = inboxSetupViewOwnership(nextSetup);
      if (!actor?.current() || !own?.current() || own.identity !== nextIdentity)
        return false;
      if (identity === nextIdentity && setup === nextSetup) return true;
      stopWork();
      identityOff();
      identity = nextIdentity;
      setup = nextSetup;
      identityOff = subscribeIdentityInvalidation(nextIdentity, () => {
        if (!actor.current()) {
          stopWork();
          copy = 'Connection changed. Your unsent text stays in memory.';
        }
      });
      return true;
    },
    send,
    stop(review) {
      if (review !== 'reviewed_product_request_cancel' || !current())
        return false;
      stopWork(true);
      if (state !== 'completed') {
        state = attempted ? 'needs_action' : 'idle';
        copy =
          'Sending is paused. Your unsent text stays in this page; existing encrypted work requires its own recovery review.';
      }
      return true;
    },
    request() {
      if (!current()) return 'invalid';
      if (!snapshot()?.dirty) return 'allowed';
      requestedRevision = productEnquirySnapshot(enquiry)?.revision;
      return 'confirmation_required';
    },
    keep() {
      if (!current()) return false;
      requestedRevision = undefined;
      return true;
    },
    discard(review) {
      if (
        !current() ||
        review !== 'reviewed_discard_private_text' ||
        requestedRevision === undefined
      )
        return 'invalid';
      const expected = requestedRevision;
      requestedRevision = undefined;
      if (expected !== productEnquirySnapshot(enquiry)?.revision)
        return 'review_required';
      close();
      return 'allowed';
    },
    unload(event) {
      if (!current() || !snapshot()?.dirty) return false;
      event.preventDefault();
      return true;
    },
    close
  });
  return token;
}
export function productRequestSnapshot(token: ProductRequestController) {
  return controllers.get(token)?.snapshot();
}
export function productRequestText(token: ProductRequestController) {
  return controllers.get(token)?.text();
}
export function updateProductRequestText(
  token: ProductRequestController,
  text: unknown,
  review: unknown
) {
  return controllers.get(token)?.update(text, review) ?? false;
}
export function configureProductRequest(
  token: ProductRequestController,
  identity: IdentitySession,
  setup: InboxSetupView,
  review: unknown
) {
  return controllers.get(token)?.configure(identity, setup, review) ?? false;
}
export function sendProductRequest(
  token: ProductRequestController,
  review: unknown
): Promise<Result> {
  return (
    controllers.get(token)?.send(review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function requestProductRequestNavigation(
  token: ProductRequestController
): NavigationResult {
  return controllers.get(token)?.request() ?? 'invalid';
}
export function keepProductRequestEditing(token: ProductRequestController) {
  return controllers.get(token)?.keep() ?? false;
}
export function discardProductRequestNavigation(
  token: ProductRequestController,
  review: unknown
): NavigationResult {
  return controllers.get(token)?.discard(review) ?? 'invalid';
}
export function productRequestBeforeUnload(
  token: ProductRequestController,
  event: BeforeUnloadEvent
) {
  return controllers.get(token)?.unload(event) ?? false;
}
export function closeProductRequestController(token: ProductRequestController) {
  controllers.get(token)?.close();
}
export function stopProductRequestWork(
  token: ProductRequestController,
  review: unknown
) {
  return controllers.get(token)?.stop(review) ?? false;
}
