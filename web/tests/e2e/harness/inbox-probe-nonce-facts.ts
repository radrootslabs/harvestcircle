import { makeRetryFixture } from './inbox-probe-retry.ts';
import {
  captureInboxProbeNonceReader,
  captureRecoveredInboxProbeNonceReader,
  readInboxProbeNonce,
  readRecoveredInboxProbeNonce,
  stopInboxProbeNonceReader,
  stopRecoveredInboxProbeNonceReader,
  expireInboxProbeNonceReaderWait,
  expireRecoveredInboxProbeNonceReaderWait,
  type InboxProbeNonceReader,
  type RecoveredInboxProbeNonceReader,
  type InboxProbeNonceProof,
  type RecoveredInboxProbeNonceProof
} from '../../../src/lib/nostr/self-recovery-reader.ts';
import { inboxProbeNonceVerificationEvidence } from '../../../src/lib/nostr/self-recovery-reader.ts';
import { inboxProbeReadbackAttemptEvidence } from '../../../src/lib/nostr/private-pool.ts';
import {
  decodePrivateRecord,
  privateRecordWire,
  type PrivateAccessProbeReceipt
} from '../../../src/lib/persistence/private-records.ts';
import {
  commitInboxProbeNonceVerification,
  inboxProbeNonceAcknowledgementBinding,
  type InboxProbeNonceAcknowledgement
} from '../../../src/lib/persistence/inbox-probes.ts';
import { closePrivateSession } from '../../../src/lib/runtime/private-session.ts';
import type { InboxProbeReadbackAttemptResult } from '../../../src/lib/nostr/private-pool.ts';
export * from './inbox-probe-retry.ts';
export * from '../../../src/lib/nostr/self-recovery-reader.ts';
export * from '../../../src/lib/persistence/inbox-probes.ts';
export { createPrivateStorageRepository } from '../../../src/lib/persistence/private-storage.ts';
export { privateRecordSettlement } from '../../../src/lib/persistence/local-inventory.ts';
export { inboxProbeReadbackAttemptEvidence } from '../../../src/lib/nostr/private-pool.ts';
export { capturePrivateOutboxCleanup } from '../../../src/lib/persistence/private-outbox-cleanup.ts';
export {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
export { createPrivateSession } from '../../../src/lib/runtime/private-session.ts';
export {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';

export async function makeNonceFactsFixture() {
  const base = await makeRetryFixture();
  let recovered = false,
    observation: InboxProbeReadbackAttemptResult | undefined,
    reader: InboxProbeNonceReader | RecoveredInboxProbeNonceReader | undefined,
    proof: InboxProbeNonceProof | RecoveredInboxProbeNonceProof | undefined,
    acknowledgement: InboxProbeNonceAcknowledgement | undefined;
  const restores = new Set<() => void>();
  function retain(off: () => void) {
    restores.add(off);
    return () => {
      off();
      restores.delete(off);
    };
  }
  function capture(review: unknown = 'reviewed_inbox_probe_nonce_decryption') {
    if (!observation) return undefined;
    const a = base.actors();
    return (
      recovered
        ? captureRecoveredInboxProbeNonceReader
        : captureInboxProbeNonceReader
    )(
      base.repository,
      a.identity,
      a.session,
      base.host.policy,
      observation,
      review
    );
  }
  function stop(expired = false) {
    if (!reader) return;
    if (recovered) {
      if (expired)
        expireRecoveredInboxProbeNonceReaderWait(
          reader as RecoveredInboxProbeNonceReader
        );
      else
        stopRecoveredInboxProbeNonceReader(
          reader as RecoveredInboxProbeNonceReader
        );
    } else if (expired)
      expireInboxProbeNonceReaderWait(reader as InboxProbeNonceReader);
    else stopInboxProbeNonceReader(reader as InboxProbeNonceReader);
  }
  function lose(kind: 'reader' | 'expiry' | 'controller' | 'head' | 'session') {
    if (kind === 'reader' || kind === 'expiry') stop(kind === 'expiry');
    else if (kind === 'head') base.actors().churnHead();
    else if (kind === 'session') closePrivateSession(base.actors().session);
    else base.stop();
  }
  return {
    ...base,
    observation: () => observation,
    nonceReader: () => reader,
    nonceProof: () => proof,
    nonceAcknowledgement: () => acknowledgement,
    captureNonce: capture,
    lose,
    async prepareRead(useRecovered: boolean, apply = true) {
      stop();
      reader = undefined;
      proof = undefined;
      acknowledgement = undefined;
      recovered = useRecovered;
      if (recovered) {
        await base.freshSession();
        if (
          (await base.recover())?.status !== 'recovered' ||
          (await base.adopt()).status !== 'adopted' ||
          !(await base.captureAction())
        )
          throw Error('actual recovered original Retry setup');
      }
      const pub = recovered
        ? await base.publish()
        : await base.originalPublish();
      if (
        pub?.status !== 'accepted' ||
        (await base.append(pub)).status !== 'saved'
      )
        throw Error('actual original archive effect and native append');
      observation = recovered ? await base.read() : await base.originalRead();
      if (observation?.status !== 'readback')
        throw Error('actual original B2 readback');
      if (apply && (await base.appendRead(observation)).status !== 'saved')
        throw Error('actual original B2 native append');
      return observation;
    },
    applyRead: () => observation && base.appendRead(observation),
    async nonce(selected = (reader ??= capture())) {
      if (!selected) return { status: 'invalid' as const };
      reader ??= selected;
      const result = recovered
        ? await readRecoveredInboxProbeNonce(
            selected as RecoveredInboxProbeNonceReader
          )
        : await readInboxProbeNonce(selected as InboxProbeNonceReader);
      if ('proof' in result) proof = result.proof;
      return result;
    },
    async appendNonce() {
      if (!proof) throw Error('actual nonce proof required');
      const result = await commitInboxProbeNonceVerification(
        base.repository,
        base.actors().session,
        base.host.policy,
        proof
      );
      if (result.acknowledgement) acknowledgement = result.acknowledgement;
      return result;
    },
    nonceBinding: () =>
      acknowledgement &&
      inboxProbeNonceAcknowledgementBinding(
        base.repository,
        base.actors().session,
        acknowledgement,
        base.host.policy
      ),
    async storedRow() {
      const row = (await base.host.stored()).find(
        (x) => x.id === base.command()
      );
      if (!row) throw Error('actual selected native row missing');
      return row;
    },
    async handwrittenNonceSuccessor() {
      if (!proof || !observation)
        throw Error('actual genuine nonce and read evidence required');
      const evidence = inboxProbeNonceVerificationEvidence(
          proof,
          base.repository,
          base.actors().session,
          base.host.policy
        ),
        effect = inboxProbeReadbackAttemptEvidence(observation),
        row = (await base.host.stored()).find((x) => x.id === base.command());
      if (!evidence || !effect || !row)
        throw Error('actual current fixture source associations');
      const record = JSON.parse(row.wire) as {
          revision: number;
          receipts: PrivateAccessProbeReceipt[];
        },
        fact = JSON.parse(effect.receiptWire) as PrivateAccessProbeReceipt,
        candidate = decodePrivateRecord(
          JSON.stringify({
            ...record,
            revision: record.revision + 1,
            receipts: [
              ...record.receipts,
              {
                ...fact,
                status: 'nonce_verified',
                observedAtMilliseconds: evidence.verifiedAtMilliseconds
              }
            ]
          }),
          row.owner,
          row.id
        );
      if (!candidate.ok)
        throw Error('actual strict codec handwritten successor');
      const wire = privateRecordWire(candidate.value, row.owner, row.id);
      if (!wire) throw Error('actual strict canonical successor wire');
      return { ...row, wire };
    },
    nativeReadonlyHook(
      hit: number,
      kind: 'before_abort' | 'after_abort' | 'callback',
      callback = () => {}
    ) {
      const original = Object.getOwnPropertyDescriptor(
        IDBIndex.prototype,
        'openCursor'
      )!.value as IDBIndex['openCursor'];
      let hits = 0,
        faults = 0;
      IDBIndex.prototype.openCursor = function (
        ...args: Parameters<IDBIndex['openCursor']>
      ) {
        const request = original.apply(this, args);
        if (
          this.objectStore.name === 'private_sends' &&
          this.objectStore.transaction.mode === 'readonly' &&
          ++hits === hit
        ) {
          faults++;
          const tx = this.objectStore.transaction;
          if (kind === 'before_abort') tx.abort();
          else if (kind === 'after_abort')
            request.addEventListener('success', () => {
              if (!request.result) tx.abort();
            });
          else callback();
        }
        return request;
      };
      return {
        faults: () => faults,
        restore: retain(() => {
          IDBIndex.prototype.openCursor = original;
        })
      };
    },
    nativeWriteHook(callback: () => void) {
      const original = Object.getOwnPropertyDescriptor(
        IDBObjectStore.prototype,
        'put'
      )!.value as IDBObjectStore['put'];
      let hits = 0;
      IDBObjectStore.prototype.put = function (
        ...args: Parameters<IDBObjectStore['put']>
      ) {
        const request = original.apply(this, args);
        if (this.name === 'private_sends' && !hits++)
          request.addEventListener('success', callback, { once: true });
        return request;
      };
      return {
        hits: () => hits,
        restore: retain(() => {
          IDBObjectStore.prototype.put = original;
        })
      };
    },
    clockHook(callback: () => void) {
      const original = Date.now;
      let hits = 0;
      Date.now = () => {
        if (!hits++) callback();
        return original();
      };
      return {
        hits: () => hits,
        restore: retain(() => {
          Date.now = original;
        })
      };
    },
    close() {
      for (const off of restores) off();
      restores.clear();
      stop();
      base.close();
    }
  };
}
