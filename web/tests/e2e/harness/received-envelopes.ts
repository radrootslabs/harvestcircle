import { makeFixture as makeRecoveryFixture } from './resume-preparation.ts';
import {
  createPrivateSession,
  closePrivateSession
} from '../../../src/lib/runtime/private-session.ts';
import {
  admitInboxEnvelope,
  retainInboxEnvelope
} from '../../../src/lib/persistence/inbox-envelope-repository.ts';
import {
  loadPrivateRecord,
  commitPrivateRecord
} from '../../../src/lib/persistence/private-storage.ts';
import {
  privateRecordSnapshot,
  decodePrivateRecord
} from '../../../src/lib/persistence/private-records.ts';
import {
  captureReceivedUnwrap,
  unwrapReceivedEnvelope,
  stopReceivedUnwrap,
  receivedNestedEnvelopeSnapshot,
  type ReceivedUnwrap,
  type ReceivedNestedEnvelope
} from '../../../src/lib/nostr/unwrap-admission.ts';
export async function makeFixture() {
  const f = await makeRecoveryFixture(),
    privateSession = await createPrivateSession(
      f.identity,
      'reviewed_private_session'
    );
  if (!privateSession) throw Error('missing genuine private session');
  const self = (await f.row())!.record.self,
    admitted = admitInboxEnvelope(
      self.wire,
      f.owner,
      'wss://archive.example.org',
      100
    );
  if (!admitted) throw Error('missing admitted real outer');
  const saved = await retainInboxEnvelope(
    f.repository,
    privateSession,
    admitted
  );
  if (saved.status !== 'retained' && saved.status !== 'duplicate')
    throw Error('no actual retained outer');
  let reader: ReceivedUnwrap | undefined,
    proof: ReceivedNestedEnvelope | undefined;
  let releaseKey: (() => void) | undefined;
  let restoreKey = () => {};
  return {
    ...f,
    outerId: self.eventId,
    capture(review: unknown = 'reviewed_inbox_unlock') {
      reader = captureReceivedUnwrap(
        f.repository,
        f.identity,
        self.eventId,
        review
      );
      return !!reader;
    },
    async run() {
      if (!reader) return { status: 'invalid' as const };
      const result = await unwrapReceivedEnvelope(reader);
      if (result.status === 'authenticated') proof = result.envelope;
      return {
        status: result.status,
        snapshot: proof && receivedNestedEnvelopeSnapshot(proof)
      };
    },
    stop() {
      if (reader) stopReceivedUnwrap(reader);
    },
    snapshot() {
      return proof && receivedNestedEnvelopeSnapshot(proof);
    },
    forged() {
      return receivedNestedEnvelopeSnapshot({} as ReceivedNestedEnvelope);
    },
    holdFinalKey() {
      const provider = window.nostr as
        { getPublicKey: () => Promise<string> } | undefined;
      if (!provider) throw Error('missing actual fixture provider');
      const original = provider.getPublicKey;
      let calls = 0;
      provider.getPublicKey = async () => {
        const owner = await original.call(provider);
        calls++;
        return calls === 3
          ? new Promise<string>((resolve) => {
              releaseKey = () => resolve(owner);
            })
          : owner;
      };
      restoreKey = () => {
        provider.getPublicKey = original;
      };
    },
    pendingFinalKey: () => !!releaseKey,
    settleFinalKey() {
      const release = releaseKey;
      releaseKey = undefined;
      release?.();
    },
    countsValue: () => f.counts(),
    async stored() {
      const row = await loadPrivateRecord(
        f.repository,
        'received_envelopes',
        self.eventId
      );
      return row.ok
        ? privateRecordSnapshot(row.value, f.owner, self.eventId)
        : undefined;
    },
    async changeObservedSource() {
      const old = await loadPrivateRecord(
        f.repository,
        'received_envelopes',
        self.eventId
      );
      if (!old.ok) throw Error('missing actual received row');
      const record = privateRecordSnapshot(old.value, f.owner, self.eventId);
      if (!record || record.family !== 'received_envelope')
        throw Error('wrong received row');
      const next = decodePrivateRecord(
        JSON.stringify({
          ...record,
          revision: record.revision + 1,
          sources: [...record.sources, 'wss://peer-inbox.example.org']
        }),
        f.owner,
        self.eventId
      );
      if (!next.ok) throw Error('invalid controlled metadata successor');
      return commitPrivateRecord(f.repository, next.value, old.value);
    },
    close() {
      releaseKey?.();
      releaseKey = undefined;
      restoreKey();
      if (reader) stopReceivedUnwrap(reader);
      closePrivateSession(privateSession);
      f.close();
    }
  };
}
