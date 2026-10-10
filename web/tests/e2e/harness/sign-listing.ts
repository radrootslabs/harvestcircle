import { makeFixture, type Mode } from './approved-signing.ts';
import { fixturePolicy, discovery, inbox } from './inbox-setup.ts';
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
import {
  publicRecordSnapshot,
  decodePublicRecord
} from '../../../src/lib/persistence/records.ts';
import type { PublicOperationRecord } from '../../../src/lib/contracts/local-records.ts';
import {
  beginListingSigning,
  runListingSigning,
  stopListingSigning,
  listingSigningSnapshot,
  listingPublicationSnapshot
} from '../../../src/lib/publishing/sign-listing.ts';

export async function createFixture() {
  const signer = makeFixture(10050),
    identity = createIdentitySession(),
    policy = fixturePolicy();
  const preference = signer.sign({
    ...signer.template,
    kind: 10050,
    created_at: 100,
    tags: [['relay', inbox]],
    content: ''
  });
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  let signs = 0,
    keys = 0,
    mode: Mode = 'exact',
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
          sink({ type: 'EOSE', from: discovery, id: 'fixture' });
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
  return {
    async exact() {
      const a = invoke(),
        b = invoke(),
        same = a === b;
      const result = await a,
        row = await stored();
      const plan =
        result.status === 'prepared'
          ? listingPublicationSnapshot(result.publication)
          : undefined;
      const retry = await invoke();
      return {
        status: result.status,
        same,
        signatures: signs,
        keys,
        revision: row.revision,
        artifact: row.artifact,
        plan,
        expectedHash: preview.expectedHash,
        originalCapture: row.capture.wire === preview.templateWire,
        retry: retry.status,
        sameArtifact:
          retry.status === 'prepared' &&
          listingPublicationSnapshot(retry.publication)?.wire === plan?.wire
      };
    },
    async wrong(next: Mode) {
      mode = next;
      const result = await invoke(),
        row = await stored();
      const retry = await invoke();
      return {
        status: result.status,
        signatures: signs,
        revision: row.revision,
        artifact: row.artifact,
        retry: retry.status,
        exposed: result.status === 'prepared'
      };
    },
    async changedKey() {
      const other = makeFixture(10050);
      observedOwner = other.owner;
      other.close();
      const result = await invoke(),
        row = await stored();
      return {
        status: result.status,
        signatures: signs,
        keys,
        revision: row.revision,
        artifact: row.artifact,
        exposed: result.status === 'prepared'
      };
    },
    async late(logout: boolean, retireSetup = false) {
      let release = () => {},
        entered = () => {};
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
      const pending = invoke();
      await enteredPromise;
      if (retireSetup) closeInboxSetupView(setup);
      else stopListingSigning(originalSigning);
      if (logout) disconnectIdentity(identity);
      release();
      const result = await pending,
        row = await stored();
      return {
        status: result.status,
        signatures: signs,
        revision: row.revision,
        artifact: row.artifact,
        exposed: result.status === 'prepared',
        snapshot: listingSigningSnapshot(originalSigning),
        originalOwner: signer.owner,
        expectedHash: preview.expectedHash,
        command
      };
    },
    async fault(
      revision: 1 | 2,
      unknown: boolean,
      unavailableReadback = false,
      tamper: 'none' | 'valid' | 'corrupt' = 'none'
    ) {
      const put = Object.getOwnPropertyDescriptor(
          IDBObjectStore.prototype,
          'put'
        ),
        abort = Object.getOwnPropertyDescriptor(
          IDBTransaction.prototype,
          'abort'
        );
      const get = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'get'
      );
      if (!put || !abort || !get) throw Error('native descriptors missing');
      const nativePut = put.value as IDBObjectStore['put'];
      const nativeGet = get.value as IDBObjectStore['get'];
      let faultPuts = 0,
        nativePuts = 0;
      Object.defineProperty(IDBObjectStore.prototype, 'put', {
        ...put,
        value: function (
          this: IDBObjectStore,
          ...args: Parameters<IDBObjectStore['put']>
        ) {
          const value: unknown = args[0];
          const wire =
            typeof value === 'object' &&
            value &&
            'wire' in value &&
            typeof value.wire === 'string'
              ? value.wire
              : undefined;
          const row = wire
            ? (JSON.parse(wire) as { revision: number })
            : undefined;
          if (this.name === 'public_operations' && row?.revision === revision) {
            faultPuts++;
            if (unknown) {
              const request = nativePut.apply(this, args);
              nativePuts++;
              if (tamper !== 'none')
                request.addEventListener('success', () => {
                  let changedWire = '{"schema":999}';
                  if (tamper === 'valid') {
                    const originalRow = JSON.parse(
                      wire!
                    ) as PublicOperationRecord;
                    const different = JSON.parse(
                      preview.templateWire
                    ) as typeof signer.template;
                    different.tags = different.tags.map((tag) =>
                      tag[0] === 'd' ? ['d', crypto.randomUUID()] : tag
                    );
                    const signed = signer.sign(different);
                    const templateWire = JSON.stringify({
                      pubkey: signed.pubkey,
                      kind: signed.kind,
                      created_at: signed.created_at,
                      tags: signed.tags,
                      content: signed.content
                    });
                    changedWire = JSON.stringify({
                      ...originalRow,
                      capture: {
                        ...originalRow.capture,
                        wire: templateWire,
                        hash: signed.id
                      },
                      artifact: {
                        eventId: signed.id,
                        wire: JSON.stringify(signed)
                      }
                    });
                    if (
                      !decodePublicRecord(changedWire, signer.owner, command).ok
                    )
                      throw Error('actual changed signed capture is not valid');
                  }
                  nativePut.call(this, {
                    owner: signer.owner,
                    id: command,
                    wire: changedWire
                  });
                  nativePuts++;
                  faultPuts++;
                });
              throw Error('HC_TEST_ONLY actual put lost acknowledgement');
            }
            this.transaction.abort();
            throw Error('HC_TEST_ONLY actual definite rollback');
          }
          const request = nativePut.apply(this, args);
          if (this.name === 'public_operations') nativePuts++;
          return request;
        }
      });
      if (unknown)
        Object.defineProperty(IDBTransaction.prototype, 'abort', {
          ...abort,
          value() {
            throw Error('HC_TEST_ONLY unavailable abort channel');
          }
        });
      if (unavailableReadback)
        Object.defineProperty(IDBObjectStore.prototype, 'get', {
          ...get,
          value: function (
            this: IDBObjectStore,
            ...args: Parameters<IDBObjectStore['get']>
          ) {
            if (this.name === 'public_operations' && faultPuts > 0)
              throw Error(
                'HC_TEST_ONLY actual readback unavailable after committed signed artifact'
              );
            return nativeGet.apply(this, args);
          }
        });
      let result;
      try {
        result = await invoke();
      } finally {
        Object.defineProperty(IDBObjectStore.prototype, 'put', put);
        Object.defineProperty(IDBTransaction.prototype, 'abort', abort);
        Object.defineProperty(IDBObjectStore.prototype, 'get', get);
      }
      const observed = await readPublicListingIntent(
        originalRepository,
        command
      );
      const row = tamper === 'corrupt' ? undefined : await stored();
      let corruptWire: string | undefined;
      if (tamper === 'corrupt')
        corruptWire = await new Promise<string>((resolve, reject) => {
          const tx = browserDatabaseTransaction(
            database,
            ['public_operations'],
            'readonly'
          );
          const request = tx
            .objectStore('public_operations')
            .get([signer.owner, command]);
          let wire: string | undefined;
          request.addEventListener('success', () => {
            const raw: unknown = request.result;
            if (
              typeof raw === 'object' &&
              raw &&
              'wire' in raw &&
              typeof raw.wire === 'string'
            )
              wire = raw.wire;
          });
          tx.addEventListener('complete', () =>
            wire === undefined
              ? reject(Error('actual corrupt row missing'))
              : resolve(wire)
          );
          tx.addEventListener('abort', () =>
            reject(Error('actual raw read aborted'))
          );
        });
      const plan =
        result.status === 'prepared'
          ? listingPublicationSnapshot(result.publication)
          : undefined;
      let retryStatus: string | undefined,
        retryPuts = 0,
        retryExact = false;
      if (unavailableReadback || tamper !== 'none') {
        Object.defineProperty(IDBObjectStore.prototype, 'put', {
          ...put,
          value: function (
            this: IDBObjectStore,
            ...args: Parameters<IDBObjectStore['put']>
          ) {
            if (this.name === 'public_operations') retryPuts++;
            return nativePut.apply(this, args);
          }
        });
        try {
          const retry = await invoke();
          retryStatus = retry.status;
          retryExact =
            retry.status === 'prepared' &&
            listingPublicationSnapshot(retry.publication)?.wire ===
              row?.artifact?.wire;
        } finally {
          Object.defineProperty(IDBObjectStore.prototype, 'put', put);
        }
      }
      return {
        status: result.status,
        exposed: !!plan,
        signatures: signs,
        revision: row?.revision,
        artifact: row?.artifact,
        faultPuts,
        nativePuts,
        readStatus: observed.status,
        corruptWire,
        retryStatus,
        retryPuts,
        retryExact,
        sameArtifact: !!plan && plan.wire === row?.artifact?.wire,
        originalCapture: row?.capture.wire === preview.templateWire
      };
    },
    close() {
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
