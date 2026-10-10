import { subscribeListingSigning } from '../../../src/lib/publishing/sign-listing.ts';
import { mount, unmount, type ComponentProps } from 'svelte';
import OperationStatus from '../../../src/lib/components/OperationStatus.svelte';
import {
  getPublicPool,
  closePublicPool,
  type PublicPool
} from '../../../src/lib/nostr/public-pool.ts';
import { deliverListing } from '../../../src/lib/publishing/deliver-listing.ts';
import { makeFixture } from './approved-signing.ts';
import { fixturePolicy, discovery, inbox } from './inbox-setup.ts';
import {
  readRelayPolicy,
  publicRelayTargets,
  validateRelayPolicy
} from '../../../src/lib/config/relays.ts';
import {
  readListingOperationView,
  listingOperationSnapshot,
  closeListingOperationView,
  type ListingOperationView
} from '../../../src/lib/publishing/listing-operation-view.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../../src/lib/nostr/request-scope.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import {
  createInboxSetupView,
  checkInboxSetupView,
  unlockInboxSetupView,
  closeInboxSetupView
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import { captureNewListingReadiness } from '../../../src/lib/publishing/new-listing-readiness.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  createPublicPublicationRepository,
  publicPublicationDraftRepository,
  readPublicListingIntent
} from '../../../src/lib/persistence/public-publications.ts';
import {
  createPublicDraftController,
  closePublicDraftController
} from '../../../src/lib/publishing/draft-controller.ts';
import {
  reviewNewListing,
  reserveNewListing,
  listingReviewSnapshot
} from '../../../src/lib/publishing/prepare-listing.ts';
import { publicRecordSnapshot } from '../../../src/lib/persistence/records.ts';
import type { PublicOperationRecord } from '../../../src/lib/contracts/local-records.ts';
import {
  beginListingSigning,
  runListingSigning,
  stopListingSigning
} from '../../../src/lib/publishing/sign-listing.ts';

export async function createFixture() {
  const signer = makeFixture(10050),
    identity = createIdentitySession(),
    policy = validateRelayPolicy(
      JSON.stringify({
        ...readRelayPolicy(fixturePolicy()),
        public: [
          ...readRelayPolicy(fixturePolicy()).public,
          {
            origin: 'wss://second.example.org',
            read: true,
            write: true,
            nip50: false
          }
        ]
      })
    );
  if (!policy) throw Error('controlled policy unavailable');
  const preference = signer.sign({
    ...signer.template,
    kind: 10050,
    created_at: 100,
    tags: [['relay', inbox]],
    content: ''
  });
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  let signs = 0,
    keys = 0;
  const mode = 'exact' as const,
    observedOwner = signer.owner;
  let beforeSign: (() => Promise<void>) | undefined;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () => {
        keys++;
        return Promise.resolve(observedOwner);
      },
      signEvent: async (input: Parameters<typeof signer.sign>[0]) => {
        signs++;
        await beforeSign?.();
        return signer.sign(input, mode);
      },
      nip44: {
        encrypt: (_peer: string, text: string) =>
          Promise.resolve('fixture:' + text),
        decrypt: (_peer: string, text: string) => Promise.resolve(text.slice(8))
      }
    }
  });
  await connectIdentity(identity);
  await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const scheduler = createPublicScheduler({
    now: () => 0,
    schedule: () => () => {}
  });
  const run = createPublicRun(scheduler, policy);
  const resolver = createInboxResolver(
    run,
    signer.owner,
    (next) =>
      openInboxRequest(
        run,
        signer.owner,
        (sink) => {
          sink({
            type: 'EVENT',
            from: discovery,
            id: 'fixture',
            event: preference
          });
          for (const origin of publicRelayTargets(policy, 'read'))
            sink({ type: 'EOSE', from: origin, id: 'fixture' });
          return () => {};
        },
        next
      ),
    () => true
  );
  // HC_TEST_ONLY_QUALIFICATION_PORT: original SDK owners, controlled exercised
  // access observation; no real extension/operator/relay qualification.
  const setup = createInboxSetupView({
    identity,
    policy,
    lookup: () => Promise.resolve(resolver),
    observeAccess: (context) => ({
      ...context,
      readTargets: context.readTargets.slice(),
      archiveTargets: context.archiveTargets.slice(),
      receive: 'qualified_exercised',
      archive: 'qualified_exercised',
      current: () => true
    })
  });
  if (!setup) throw Error('original setup missing');
  await checkInboxSetupView(setup);
  await unlockInboxSetupView(setup);
  const ready = captureNewListingReadiness(
    identity,
    setup,
    'reviewed_new_listing_inbox'
  );
  if (!ready) throw Error('original readiness missing');
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const database = opened.owner,
    repository = createPublicPublicationRepository(database, signer.owner);
  const drafts = repository && publicPublicationDraftRepository(repository);
  if (!repository || !drafts) throw Error('original repository missing');
  const originalRepository = repository;
  const controller = await createPublicDraftController(drafts, {
    title: 'Carrots',
    description: 'Exact signed description',
    location: 'Local farm',
    amount: '2',
    currency: 'USD',
    unit: 'kg',
    quantity: '',
    contactType: '',
    contactValue: ''
  });
  if (!controller) throw Error('original controller missing');
  const command = crypto.randomUUID();
  const reviewed = await reviewNewListing(
    repository,
    controller,
    ready,
    command,
    'reviewed_new_listing'
  );
  if (reviewed.status !== 'review') throw Error(reviewed.reason);
  const original = reviewed.review,
    preview = listingReviewSnapshot(original);
  if (!preview) throw Error('original preview missing');
  const reserved = await reserveNewListing(
    original,
    'publish_reviewed_listing'
  );
  if (reserved.status !== 'saved') throw Error(reserved.status);
  const signing = beginListingSigning(original, 'sign_reviewed_listing');
  if (!signing) throw Error('original signing missing');
  const originalSigning = signing;
  async function stored() {
    const read = await readPublicListingIntent(originalRepository, command);
    const row =
      read.status === 'found' &&
      publicRecordSnapshot(read.record, signer.owner, command);
    if (!row || row.family !== 'public_operation')
      throw Error('original stored command missing');
    return row;
  }
  function invoke() {
    return runListingSigning(originalSigning, 'sign_reviewed_listing');
  }
  let mounted: ReturnType<typeof mount> | undefined;
  let target: HTMLElement | undefined;
  let view: ListingOperationView | undefined, pool: PublicPool | undefined;
  let pendingSigning: ReturnType<typeof invoke> | undefined;
  let release = () => {};
  async function show(original = true) {
    const next = await readListingOperationView(
      originalRepository,
      identity,
      command,
      original ? originalSigning : undefined
    );
    if (next.status !== 'found') return next;
    view = next.view;
    return listingOperationSnapshot(view);
  }
  function nativeWrite(
    store: 'public_operations' | 'public_drafts',
    action: (value: IDBObjectStore) => void
  ) {
    const transaction = browserDatabaseTransaction(
      database,
      [store],
      'readwrite'
    );
    return new Promise<void>((resolve, reject) => {
      transaction.addEventListener('complete', () => resolve());
      transaction.addEventListener('abort', () =>
        reject(Error('controlled transaction abort'))
      );
      action(transaction.objectStore(store));
    });
  }
  return {
    owner: signer.owner,
    command,
    show,
    async renderView(copied = false) {
      if (mounted) await unmount(mounted);
      target?.remove();
      target = document.createElement('div');
      target.id = 'hcp114-operation';
      document.body.append(target);
      const value = await show();
      if (!view) throw Error('original view missing');
      mounted = mount(OperationStatus, {
        target,
        props: {
          publicOperation: copied ? { ...view } : view
        } as ComponentProps<typeof OperationStatus>
      });
      return value;
    },
    snapshot() {
      return view && listingOperationSnapshot(view);
    },
    copiedSnapshot() {
      return view && listingOperationSnapshot({ ...view });
    },
    async row() {
      return await stored();
    },
    async signed() {
      return (await invoke()).status;
    },
    async deliver() {
      const signed = await invoke();
      if (signed.status !== 'prepared') return signed.status;
      pool = getPublicPool(policy);
      if (!pool) throw Error('original pool unavailable');
      return (
        await deliverListing(
          signed.publication,
          pool,
          policy,
          'publish_stored_listing'
        )
      ).status;
    },
    async holdSigning() {
      let entered = () => {};
      const enteredPromise = new Promise<void>((resolve) => {
        entered = resolve;
      });
      const held = new Promise<void>((resolve) => {
        release = resolve;
      });
      beforeSign = () => {
        entered();
        return held;
      };
      pendingSigning = invoke();
      await enteredPromise;
    },
    async observerAddition() {
      let first = 0,
        added = 0,
        offAdded = () => {};
      const off = subscribeListingSigning(originalSigning, () => {
        first++;
        offAdded = subscribeListingSigning(originalSigning, () => {
          added++;
        });
      });
      stopListingSigning(originalSigning);
      off();
      offAdded();
      release();
      await pendingSigning;
      return { first, added };
    },
    async observerStop() {
      let first = 0;
      const off = subscribeListingSigning(originalSigning, () => {
        first++;
        if (first === 1) stopListingSigning(originalSigning);
      });
      stopListingSigning(originalSigning);
      off();
      release();
      await pendingSigning;
      return { first };
    },
    async releaseSigning() {
      release();
      const result = await pendingSigning;
      return result?.status;
    },
    async stopSigning() {
      stopListingSigning(originalSigning);
      release();
      const result = await pendingSigning;
      return result?.status;
    },
    async replace(row: PublicOperationRecord) {
      await nativeWrite('public_operations', (store) => {
        store.put({
          owner: signer.owner,
          id: command,
          wire: JSON.stringify(row)
        });
      });
    },
    async externalSource() {
      const row = await stored();
      if (!row.artifact) throw Error('signed original missing');
      const original = JSON.parse(row.capture.wire) as Parameters<
        typeof signer.sign
      >[0];
      const template = { ...original, created_at: original.created_at + 1 };
      const artifact = signer.sign(template);
      await nativeWrite('public_operations', (store) => {
        store.put({
          owner: signer.owner,
          id: command,
          wire: JSON.stringify({
            ...row,
            revision: 2,
            source: { type: 'public_head', wire: row.artifact!.wire },
            capture: {
              ...row.capture,
              wire: JSON.stringify(template),
              hash: artifact.id
            },
            artifact: { eventId: artifact.id, wire: JSON.stringify(artifact) },
            receipts: []
          })
        });
      });
    },
    async removeDraft() {
      await nativeWrite('public_drafts', (store) => {
        store.delete([signer.owner, preview.draftId]);
      });
    },
    async evidence(
      status: 'accepted' | 'refused' | 'timed_out' | 'unknown' | 'stopped',
      target: number,
      readback: boolean
    ) {
      const row = await stored();
      if (!row.artifact) throw Error('original artifact missing');
      const receipt = {
        actionId: crypto.randomUUID(),
        origin: row.capture.targets[target],
        role: 'publication' as const,
        attempt: 1,
        eventId: row.artifact.eventId,
        status,
        observedAtMilliseconds: Date.now(),
        readbackWire: readback ? row.artifact.wire : null
      };
      // HC_TEST_ONLY stored observation metadata, never an actual relay qualification claim.
      await nativeWrite('public_operations', (store) => {
        store.put({
          owner: signer.owner,
          id: command,
          wire: JSON.stringify({
            ...row,
            revision: row.revision + 1,
            receipts: [...row.receipts, receipt]
          })
        });
      });
    },
    async supersede() {
      const old = view;
      await show();
      return old && listingOperationSnapshot(old);
    },
    async readWhileLogout() {
      const prototype = IDBTransaction.prototype,
        descriptor = Object.getOwnPropertyDescriptor(
          prototype,
          'addEventListener'
        );
      const inherited =
        descriptor ??
        Object.getOwnPropertyDescriptor(
          EventTarget.prototype,
          'addEventListener'
        );
      if (!inherited) throw Error('native registration missing');
      const native = inherited.value as IDBTransaction['addEventListener'];
      let entered = () => {},
        releaseRead = () => {},
        completed = 0;
      const enteredPromise = new Promise<void>((resolve) => {
        entered = resolve;
      });
      const restore = () => {
        if (descriptor)
          Object.defineProperty(prototype, 'addEventListener', descriptor);
        else Reflect.deleteProperty(prototype, 'addEventListener');
      };
      prototype.addEventListener = function (
        type: string,
        listener: EventListenerOrEventListenerObject,
        options?: boolean | AddEventListenerOptions
      ) {
        if (
          type !== 'complete' ||
          !listener ||
          this.mode !== 'readonly' ||
          !this.objectStoreNames.contains('public_operations')
        )
          return native.call(this, type, listener, options);
        restore();
        return native.call(
          this,
          type,
          function (event: Event) {
            const transaction = event.currentTarget;
            if (!(transaction instanceof IDBTransaction))
              throw Error('native complete missing');
            completed++;
            releaseRead = () => {
              if (typeof listener === 'function')
                listener.call(transaction, event);
              else listener.handleEvent(event);
            };
            entered();
          },
          options
        );
      };
      try {
        const pending = show();
        await enteredPromise;
        disconnectIdentity(identity);
        releaseRead();
        return { result: await pending, nativeCompletions: completed };
      } finally {
        releaseRead();
        restore();
      }
    },
    logout() {
      disconnectIdentity(identity);
    },
    closeView() {
      if (view) closeListingOperationView(view);
    },
    counts() {
      return { signs, keys };
    },
    close() {
      release();
      if (mounted) void unmount(mounted);
      target?.remove();
      if (pool) closePublicPool(pool);
      if (view) closeListingOperationView(view);
      closePublicDraftController(controller);
      closeBrowserDatabase(database);
      closeInboxSetupView(setup);
      disconnectIdentity(identity);
      closePublicScheduler(scheduler);
      signer.close();
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else Reflect.deleteProperty(window, 'nostr');
    }
  };
}

export async function reopen(owner: string, command: string) {
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  let signs = 0;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () => Promise.resolve(owner),
      signEvent: () => {
        signs++;
        return Promise.reject(Error('read-only reopen never signs'));
      }
    }
  });
  const identity = createIdentitySession();
  await connectIdentity(identity);
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error(opened.reason);
  const database = opened.owner,
    repository = createPublicPublicationRepository(database, owner);
  if (!repository) throw Error('reopened repository missing');
  const result = await readListingOperationView(repository, identity, command);
  return {
    result:
      result.status === 'found'
        ? listingOperationSnapshot(result.view)
        : result,
    counts: () => ({ signs }),
    close() {
      if (result.status === 'found') closeListingOperationView(result.view);
      closeBrowserDatabase(database);
      disconnectIdentity(identity);
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else Reflect.deleteProperty(window, 'nostr');
    }
  };
}
