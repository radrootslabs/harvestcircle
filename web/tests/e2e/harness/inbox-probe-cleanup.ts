import { makeRetryFixture } from './inbox-probe-retry.ts';
import {
  captureInboxProbeNonceReader,
  captureRecoveredInboxProbeNonceReader,
  readInboxProbeNonce,
  readRecoveredInboxProbeNonce,
  type InboxProbeNonceProof,
  type RecoveredInboxProbeNonceProof
} from '../../../src/lib/nostr/self-recovery-reader.ts';
import {
  commitInboxProbeNonceVerification,
  type InboxProbeNonceAcknowledgement
} from '../../../src/lib/persistence/inbox-probes.ts';
import {
  captureInboxProbeCleanup,
  deleteReviewedInboxProbe,
  stopInboxProbeCleanup,
  inboxProbeRemovalAcknowledgementBinding,
  type InboxProbeCleanup,
  type InboxProbeRemovalAcknowledgement
} from '../../../src/lib/persistence/inbox-probe-cleanup.ts';
import { captureInboxProbePublicationAction } from '../../../src/lib/nostr/private-publisher.ts';
import type {
  InboxProbeArchiveAttemptResult,
  InboxProbeReadbackAttemptResult
} from '../../../src/lib/nostr/private-pool.ts';
import type { PrivateAccessProbe } from '../../../src/lib/persistence/private-records.ts';
import { generateSecretKey, finalizeEvent } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import {
  decodePrivateRecord,
  privateRecordSnapshot
} from '../../../src/lib/persistence/private-records.ts';
import { privateRecordSettlement } from '../../../src/lib/persistence/local-inventory.ts';
import {
  openBrowserDatabase,
  browserDatabaseTransaction,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
import {
  captureInboxProbeSealOperation,
  buildInboxProbeSeal
} from '../../../src/lib/nostr/seal-builder.ts';
import { buildInboxProbeGiftwrap } from '../../../src/lib/nostr/giftwrap-builder.ts';
import { commitInboxProbe } from '../../../src/lib/persistence/inbox-probes.ts';
export * from './inbox-probe-nonce-facts.ts';
export * from '../../../src/lib/persistence/inbox-probe-cleanup.ts';
export {
  removePrivateInboxProbe,
  privateProbeRemovalEvidence
} from '../../../src/lib/persistence/private-storage.ts';
export {
  acquireInboxProbeCleanupLease,
  releaseInboxProbeCleanupLease,
  retireInboxProbeCleanupLease,
  inboxProbePublicationSnapshot,
  inboxProbeReadbackSnapshot
} from '../../../src/lib/nostr/private-publisher.ts';
export {
  publishInboxProbeArchiveAttempt,
  readInboxProbeAttempt
} from '../../../src/lib/nostr/private-pool.ts';
export { inboxProbeRumorWire } from '../../../src/lib/nostr/inbox-access.ts';
export { inboxAccessConstructionBinding } from '../../../src/lib/messaging/inbox-access-controller.ts';
export {
  getPrivatePool,
  closePrivatePool,
  subscribePrivatePage,
  subscribePrivateLive
} from '../../../src/lib/nostr/private-pool.ts';
export {
  beginInboxAuthentication,
  closeInboxAuthentication
} from '../../../src/lib/nostr/inbox-auth.ts';

export async function makeCleanupFixture() {
  const base = await makeRetryFixture();
  let recovered = false,
    nonce: InboxProbeNonceAcknowledgement | undefined,
    proof: InboxProbeNonceProof | RecoveredInboxProbeNonceProof | undefined,
    cleanup: InboxProbeCleanup | undefined,
    removal: InboxProbeRemovalAcknowledgement | undefined,
    observation: InboxProbeReadbackAttemptResult | undefined;
  const observations: (
      InboxProbeArchiveAttemptResult | InboxProbeReadbackAttemptResult
    )[] = [],
    restores = new Set<() => void>();
  function retain(off: () => void) {
    restores.add(off);
    return () => {
      off();
      restores.delete(off);
    };
  }
  const row = async () => {
    const found = (await base.host.stored()).find(
      (x) => x.id === base.command()
    );
    if (!found) throw Error('actual selected row missing');
    return JSON.parse(found.wire) as PrivateAccessProbe;
  };
  return {
    ...base,
    observations: () => observations.slice(),
    nonce: () => nonce,
    nonceProof: () => proof,
    cleanup: () => cleanup,
    removal: () => removal,
    observation: () => observation,
    async retryOriginalPreparation() {
      const identity = base.originalIdentity(),
        a = base.actors();
      if (!identity) throw Error('actual original identity');
      const operation = captureInboxProbeSealOperation(
        a.session,
        identity,
        'reviewed_inbox_probe_seal'
      );
      if (!operation) return { status: 'capture_refused' };
      const sealed = await buildInboxProbeSeal(operation);
      if (!('seal' in sealed)) return sealed;
      const wrapped = buildInboxProbeGiftwrap(
        sealed.seal,
        'reviewed_inbox_probe_wrap'
      );
      return wrapped
        ? commitInboxProbe(
            base.repository,
            a.session,
            wrapped,
            'reviewed_inbox_probe_commit'
          )
        : { status: 'wrap_refused' };
    },
    async seedPreservedRecords() {
      const selected = await row(),
        owner = selected.owner,
        peer =
          'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5',
        secret = generateSecretKey(),
        conversation = nip44.v2.utils.getConversationKey(secret, peer);
      let peerArtifact: { eventId: string; wire: string };
      try {
        const outer = finalizeEvent(
          {
            kind: 1059,
            created_at: selected.createdAt,
            tags: [['p', peer]],
            content: nip44.v2.encrypt(
              'storage-only preservation fixture',
              conversation
            )
          },
          secret
        );
        peerArtifact = { eventId: outer.id, wire: JSON.stringify(outer) };
      } finally {
        conversation.fill(0);
        secret.fill(0);
      }
      const route = (
        role: 'peer' | 'self_archive',
        author: string,
        origin: string
      ) => ({
        role,
        author,
        targets: [origin],
        knownBase: { author, id: 'c'.repeat(64), createdAt: 100 },
        sources: [
          {
            source: 'wss://discovery.example.org',
            state: 'eose',
            head: { id: 'c'.repeat(64), createdAt: 100 }
          }
        ]
      });
      // Strict native metadata fixtures, not sender capabilities or exchange Q.
      const ordinary = {
          schema: 1,
          family: 'private_send_operation',
          owner,
          id: crypto.randomUUID(),
          revision: 1,
          peer,
          rumorHash: selected.rumorHash,
          createdAt: selected.createdAt,
          self: selected.self!,
          peerArtifact,
          deliveryPlan: {
            state: 'prepared',
            routes: {
              peer: route('peer', peer, 'wss://peer.example.org'),
              archive: route('self_archive', owner, 'wss://archive.example.org')
            }
          },
          receipts: [
            {
              actionId: crypto.randomUUID(),
              origin: 'wss://peer.example.org',
              role: 'peer',
              attempt: 1,
              eventId: peerArtifact.eventId,
              status: 'accepted',
              observedAtMilliseconds: 1700000000000,
              readbackWire: null
            },
            {
              actionId: crypto.randomUUID(),
              origin: 'wss://archive.example.org',
              role: 'self_archive',
              attempt: 1,
              eventId: selected.self!.eventId,
              status: 'accepted',
              observedAtMilliseconds: 1700000000000,
              readbackWire: null
            }
          ]
        },
        protectedProbe = { ...selected, id: crypto.randomUUID() },
        received = {
          schema: 1,
          family: 'received_envelope',
          owner,
          id: selected.self!.eventId,
          revision: 0,
          outer: selected.self!.wire,
          observedAtMilliseconds: 1700000000000,
          sources: ['wss://inbox.example.org'],
          read: null
        };
      const values = [ordinary, protectedProbe, received].map((value) => {
        const wire = JSON.stringify(value),
          decoded = decodePrivateRecord(wire, owner, value.id);
        if (!decoded.ok) throw Error('strict preservation fixture rejected');
        const record = privateRecordSnapshot(decoded.value, owner, value.id)!;
        if (value === ordinary && privateRecordSettlement(record) !== 'settled')
          throw Error('actual settled metadata required');
        return { owner, id: value.id, wire };
      });
      const opened = await openBrowserDatabase();
      if (opened.state !== 'ready') throw Error('actual preservation database');
      const database = opened.owner;
      try {
        const tx = browserDatabaseTransaction(
          database,
          ['private_sends', 'received_envelopes'],
          'readwrite'
        );
        values.forEach((value, index) =>
          tx
            .objectStore(index === 2 ? 'received_envelopes' : 'private_sends')
            .put(value)
        );
        await new Promise<void>((resolve, reject) => {
          tx.addEventListener('complete', () => resolve());
          tx.addEventListener('abort', () =>
            reject(Error('preservation native seed abort'))
          );
        });
      } finally {
        closeBrowserDatabase(database);
      }
      return values;
    },
    async receivedRows() {
      const opened = await openBrowserDatabase();
      if (opened.state !== 'ready') throw Error('actual received database');
      try {
        const tx = browserDatabaseTransaction(
            opened.owner,
            ['received_envelopes'],
            'readonly'
          ),
          request = tx.objectStore('received_envelopes').getAll();
        return await new Promise<{ owner: string; id: string; wire: string }[]>(
          (resolve, reject) => {
            tx.addEventListener('complete', () => resolve(request.result));
            tx.addEventListener('abort', () =>
              reject(Error('received native observation abort'))
            );
          }
        );
      } finally {
        closeBrowserDatabase(opened.owner);
      }
    },
    async qualify(
      useRecovered = false,
      options: { fresh?: boolean; skip?: string[]; readOrigin?: string } = {}
    ) {
      recovered = useRecovered;
      observations.length = 0;
      nonce = undefined;
      proof = undefined;
      cleanup = undefined;
      removal = undefined;
      if (recovered) {
        if (options.fresh !== false) await base.freshSession();
        if (
          (await base.recover())?.status !== 'recovered' ||
          (await base.adopt()).status !== 'adopted' ||
          !(await base.captureAction())
        )
          throw Error('actual recovered cleanup setup');
      }
      const selected = await row();
      for (const origin of selected.binding.archiveTargets) {
        if (options.skip?.includes(origin)) continue;
        const result = await (recovered
          ? base.publish(origin)
          : base.originalPublish(origin));
        if (
          !result ||
          result.status !== 'accepted' ||
          (await base.append(result)).status !== 'saved'
        )
          throw Error('actual named archive source and native append');
        observations.push(result);
      }
      observation = await (recovered
        ? base.read(options.readOrigin)
        : base.originalRead(options.readOrigin));
      if (
        !observation ||
        observation.status !== 'readback' ||
        (await base.appendRead(observation)).status !== 'saved'
      )
        throw Error('actual source readback and native append');
      observations.push(observation);
      const a = base.actors(),
        reader = (
          recovered
            ? captureRecoveredInboxProbeNonceReader
            : captureInboxProbeNonceReader
        )(
          base.repository,
          a.identity,
          a.session,
          base.host.policy,
          observation,
          'reviewed_inbox_probe_nonce_decryption'
        );
      if (!reader) throw Error('actual nonce reader');
      const verified = recovered
        ? await readRecoveredInboxProbeNonce(
            reader as Parameters<typeof readRecoveredInboxProbeNonce>[0]
          )
        : await readInboxProbeNonce(
            reader as Parameters<typeof readInboxProbeNonce>[0]
          );
      if (!('proof' in verified))
        throw Error('actual nonce verification ' + verified.status);
      proof = verified.proof;
      const saved = await commitInboxProbeNonceVerification(
        base.repository,
        a.session,
        base.host.policy,
        proof
      );
      if (!saved.acknowledgement)
        throw Error('actual nonce native fact ' + saved.status);
      nonce = saved.acknowledgement;
      return saved;
    },
    async select(
      review: unknown = 'reviewed_inbox_probe_cleanup_selection',
      input: unknown = observations
    ) {
      cleanup = await captureInboxProbeCleanup(
        base.repository,
        base.actors().session,
        base.host.policy,
        nonce!,
        input,
        review
      );
      return cleanup;
    },
    async remove(
      review: unknown = 'reviewed_delete_local_inbox_probe',
      selected = cleanup
    ) {
      const result = await deleteReviewedInboxProbe(selected!, review);
      if (result.acknowledgement) removal = result.acknowledgement;
      return result;
    },
    stopCleanup() {
      if (cleanup) stopInboxProbeCleanup(cleanup);
    },
    removalBinding: () =>
      removal &&
      inboxProbeRemovalAcknowledgementBinding(
        removal,
        base.repository,
        base.actors().session,
        base.host.policy
      ),
    async withHeld<T>(kind: 'lock' | 'SDK', work: () => Promise<T>) {
      let release = () => {},
        enter = () => {};
      const entered = new Promise<void>((resolve) => {
          enter = resolve;
        }),
        wait = new Promise<void>((resolve) => {
          release = resolve;
        }),
        holder = async () => {
          enter();
          await wait;
        },
        held =
          kind === 'lock'
            ? navigator.locks.request(
                'harvestcircle:owner:' + base.host.owner,
                holder
              )
            : (
                await import('../../../src/lib/runtime/private-session.ts')
              ).withIdlePrivateLocalStorage(base.actors().session, holder);
      await entered;
      try {
        return await work();
      } finally {
        release();
        await held;
      }
    },
    async capacity() {
      const session = base.actors().session,
        originals = new Map<string, string>();
      for (let index = 0; index < 100; index++) {
        if (index) {
          if (
            !(await base.replaceController()) ||
            (await base.prepareOriginal()).status !== 'prepared'
          )
            throw Error('actual protected capacity preparation ' + index);
        }
        if (
          !captureInboxProbePublicationAction(
            base.repository,
            session,
            base.host.policy,
            base.originalAck()!,
            'reviewed_inbox_probe_network'
          )
        )
          throw Error('actual protected command slot ' + index);
        const selected = (await base.host.stored()).find(
          (x) => x.id === base.command()
        )!;
        originals.set(selected.id, selected.wire);
      }
      const retained = await base.host.stored();
      if (retained.length !== 100 || base.actors().session !== session)
        throw Error('actual same-session full capacity');
      await base.replaceController();
      const denied = await base.prepareOriginal();
      return { session, originals, denied };
    },
    // Actual native request/complete/abort instrumentation; no success labels.
    nativeFault(
      mode:
        | 'abort'
        | 'uncertain'
        | 'complete_loss'
        | 'read_before'
        | 'read_after'
        | 'read_loss'
        | 'delete_loss'
        | 'wire_race'
        | 'initial_read_abort'
        | 'native_pre_delete_loss',
      callback = () => {}
    ) {
      const remove = Object.getOwnPropertyDescriptor(
          IDBObjectStore.prototype,
          'delete'
        )!.value as IDBObjectStore['delete'],
        abort = Object.getOwnPropertyDescriptor(
          IDBTransaction.prototype,
          'abort'
        )!.value as IDBTransaction['abort'],
        cursor = Object.getOwnPropertyDescriptor(
          IDBIndex.prototype,
          'openCursor'
        )!.value as IDBIndex['openCursor'],
        transaction = Object.getOwnPropertyDescriptor(
          IDBDatabase.prototype,
          'transaction'
        )!.value as IDBDatabase['transaction'];
      let selected: IDBTransaction | undefined,
        readonly = 0,
        deletes = 0,
        completes = 0,
        aborts = 0,
        refused = 0,
        races = 0;
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<IDBDatabase['transaction']>
      ) {
        if (
          mode === 'wire_race' &&
          !races &&
          args[1] === 'readwrite' &&
          Array.from(args[0] as string[]).includes('private_sends')
        ) {
          races++;
          const other = transaction.call(this, ['private_sends'], 'readwrite'),
            store = other.objectStore('private_sends'),
            get = store.get([base.host.owner, base.command()!]);
          get.addEventListener('success', () => {
            const item = get.result as {
                owner: string;
                id: string;
                wire: string;
              },
              record = JSON.parse(item.wire) as { revision: number };
            record.revision++;
            store.put({ ...item, wire: JSON.stringify(record) });
          });
        }
        return transaction.apply(this, args);
      };
      IDBIndex.prototype.openCursor = function (
        ...args: Parameters<IDBIndex['openCursor']>
      ) {
        const request = cursor.apply(this, args),
          tx = this.objectStore.transaction;
        if (
          this.objectStore.name === 'private_sends' &&
          tx.mode === 'readonly'
        ) {
          readonly++;
          const hit = readonly;
          if (mode === 'read_before' && hit === 2) tx.abort();
          if (mode === 'read_after' && hit === 2)
            request.addEventListener('success', () => {
              if (!request.result) tx.abort();
            });
          if (mode === 'read_loss' && hit === 1)
            request.addEventListener('success', () => {
              if (!request.result) callback();
            });
          if (mode === 'initial_read_abort' && hit === 1) tx.abort();
        }
        if (
          this.objectStore.name === 'private_sends' &&
          tx.mode === 'readwrite' &&
          mode === 'native_pre_delete_loss'
        )
          request.addEventListener('success', () => {
            if (!request.result) callback();
          });
        return request;
      };
      IDBTransaction.prototype.abort = function (
        ...args: Parameters<IDBTransaction['abort']>
      ) {
        if (this === selected && mode === 'uncertain') {
          refused++;
          throw Error('controlled actual abort refusal');
        }
        return abort.apply(this, args);
      };
      IDBObjectStore.prototype.delete = function (
        ...args: Parameters<IDBObjectStore['delete']>
      ) {
        const request = remove.apply(this, args);
        if (this.name === 'private_sends' && !selected) {
          deletes++;
          selected = this.transaction;
          selected.addEventListener('complete', () => {
            completes++;
            if (mode === 'complete_loss') callback();
          });
          selected.addEventListener('abort', () => {
            aborts++;
          });
          request.addEventListener('success', () => {
            if (mode === 'abort') selected!.abort();
            if (mode === 'delete_loss') callback();
          });
          if (mode === 'uncertain')
            throw Error('controlled throw after actual delete admission');
        }
        return request;
      };
      return {
        counts: () => ({
          readonly,
          deletes,
          completes,
          aborts,
          refused,
          races
        }),
        restore: retain(() => {
          IDBObjectStore.prototype.delete = remove;
          IDBTransaction.prototype.abort = abort;
          IDBIndex.prototype.openCursor = cursor;
          IDBDatabase.prototype.transaction = transaction;
        })
      };
    },
    close() {
      for (const off of restores) off();
      restores.clear();
      if (cleanup) stopInboxProbeCleanup(cleanup);
      base.close();
    }
  };
}
