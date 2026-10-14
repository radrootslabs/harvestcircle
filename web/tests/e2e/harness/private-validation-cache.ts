// CR005_TEST_ONLY: real IDB/validator, no nested/extension/operator/fullQ proof.
import { finalizeEvent, generateSecretKey } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { makeFixture as makeSigner } from './approved-signing.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  createPrivateStorageRepository,
  loadPrivateRecord,
  listPrivateSendRecords,
  listPrivateReceivedRecords
} from '../../../src/lib/persistence/private-storage.ts';
import {
  decodePrivateRecord,
  privateRecordSnapshot
} from '../../../src/lib/persistence/private-records.ts';
import { readRetainedOuterMetadata } from '../../../src/lib/persistence/history-metadata-repository.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import { createPrivateSession } from '../../../src/lib/runtime/private-session.ts';
declare global {
  var __cr005VerifyCalls: number | undefined;
}
type Store = 'private_sends' | 'received_envelopes';
type Row = { owner: string; id: string; wire: string };
export async function createFixture() {
  const signer = makeSigner(),
    peer = makeSigner(),
    prior = Object.getOwnPropertyDescriptor(window, 'nostr'),
    owner = signer.owner;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () => Promise.resolve(owner),
      signEvent: (t: Parameters<typeof signer.sign>[0]) =>
        Promise.resolve(signer.sign(t)),
      nip44: {
        encrypt: (p: string, t: string) =>
          Promise.resolve(signer.encrypt(p, t)),
        decrypt: (p: string, t: string) => Promise.resolve(signer.decrypt(p, t))
      }
    }
  });
  const identity = createIdentitySession();
  await connectIdentity(identity);
  await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const session = await createPrivateSession(
      identity,
      'reviewed_private_session'
    ),
    opened = await openBrowserDatabase();
  if (!session || opened.state !== 'ready')
    throw Error('genuine fixture session/database absent');
  const database = opened.owner,
    repository = createPrivateStorageRepository(database, owner),
    foreign = createPrivateStorageRepository(database, peer.owner);
  if (!repository || !foreign) throw Error('genuine repositories absent');
  const repo = repository,
    foreignRepo = foreign,
    rows: Row[] = [],
    sends: Row[] = [],
    observed = Date.now();
  for (let index = 0; index < 2000; index++) {
    const secret = generateSecretKey(),
      conversation = nip44.v2.utils.getConversationKey(secret, owner);
    let outer;
    try {
      outer = finalizeEvent(
        {
          kind: 1059,
          created_at: Math.floor(observed / 1000) - index,
          tags: [['p', owner]],
          content: nip44.v2.encrypt(
            'CR005_TEST_ONLY_opaque_transport_seal',
            conversation
          )
        },
        secret
      );
    } finally {
      secret.fill(0);
      conversation.fill(0);
    }
    const record = {
        schema: 1,
        family: 'received_envelope',
        owner,
        id: outer.id,
        revision: 0,
        outer: JSON.stringify(outer),
        observedAtMilliseconds: observed,
        sources: ['wss://archive.example.org'],
        read: null
      },
      wire = JSON.stringify(record);
    if (!decodePrivateRecord(wire, owner, record.id).ok)
      throw Error('strict outer rejected');
    rows.push({ owner, id: record.id, wire });
  }
  for (let index = 0; index < 100; index++) {
    const id = crypto.randomUUID(),
      wire = JSON.stringify({
        schema: 1,
        family: 'private_send_reservation',
        owner,
        id,
        revision: 0,
        peer: peer.owner,
        rumorHash: 'a'.repeat(64),
        createdAt: Math.floor(observed / 1000)
      });
    if (!decodePrivateRecord(wire, owner, id).ok)
      throw Error('strict reservation rejected');
    sends.push({ owner, id, wire });
  }
  rows.sort((a, b) => a.id.localeCompare(b.id));
  const nativeCursor = Reflect.get<IDBIndex, 'openCursor'>(
    IDBIndex.prototype,
    'openCursor'
  );
  let visits = 0;
  IDBIndex.prototype.openCursor = function (...args) {
    const request = Reflect.apply(nativeCursor, this, args);
    request.addEventListener('success', () => {
      if (request.result) visits++;
    });
    return request;
  };
  async function put(store: Store, records: readonly unknown[], other = false) {
    const openedOther = other ? await openBrowserDatabase() : undefined;
    if (openedOther && openedOther.state !== 'ready')
      throw Error('second connection absent');
    const db = openedOther?.state === 'ready' ? openedOther.owner : database;
    try {
      const tx = browserDatabaseTransaction(db, [store], 'readwrite');
      for (const row of records) tx.objectStore(store).put(row);
      await new Promise<void>((resolve, reject) => {
        tx.addEventListener('complete', () => resolve());
        tx.addEventListener('abort', () =>
          reject(Error('actual fixture write aborted'))
        );
      });
    } finally {
      if (openedOther?.state === 'ready')
        closeBrowserDatabase(openedOther.owner);
    }
  }
  await put('received_envelopes', rows);
  await put('private_sends', sends);
  function reset() {
    visits = 0;
    globalThis.__cr005VerifyCalls = 0;
  }
  function measured(start: number) {
    return {
      cursorVisits: visits,
      verificationCalls: globalThis.__cr005VerifyCalls ?? 0,
      milliseconds: performance.now() - start
    };
  }
  async function sample(store: Store = 'received_envelopes') {
    reset();
    const start = performance.now(),
      row = store === 'private_sends' ? sends[0] : rows[0],
      result = await loadPrivateRecord(repo, store, row.id);
    return {
      ...measured(start),
      ok: result.ok,
      reason: result.ok ? null : result.reason,
      revision: result.ok
        ? privateRecordSnapshot(result.value, owner, row.id)?.revision
        : null
    };
  }
  return {
    owner,
    sample,
    async inventory() {
      reset();
      const start = performance.now(),
        result = await listPrivateReceivedRecords(repo);
      return {
        ...measured(start),
        ok: result.ok,
        count: result.ok ? result.value.length : null
      };
    },
    async metadata(other = false) {
      reset();
      const start = performance.now(),
        result = await readRetainedOuterMetadata(
          other ? foreignRepo : repo,
          session
        );
      return {
        ...measured(start),
        ok: result.ok,
        count: result.ok ? result.rows.length : null,
        reason: result.ok ? null : result.reason
      };
    },
    bytes: () => ({
      received: rows.reduce(
        (n, row) => n + new TextEncoder().encode(JSON.stringify(row)).length,
        0
      ),
      sends: sends.reduce(
        (n, row) => n + new TextEncoder().encode(JSON.stringify(row)).length,
        0
      )
    }),
    async changeTail(
      kind: 'valid' | 'metadata' | 'corrupt' | 'summary' | 'id',
      other = false
    ) {
      const original = rows.at(-1)!;
      let next: unknown;
      if (kind === 'summary')
        next = { ...original, untrustedSummary: { count: 0, bytes: 0 } };
      else {
        const record = JSON.parse(original.wire) as Record<string, unknown>;
        if (kind === 'valid' || kind === 'metadata') record.revision = 1;
        if (kind === 'metadata') {
          record.sources = [
            'wss://archive.example.org',
            'wss://peer.example.org'
          ];
          record.read = {
            rumorHash: 'b'.repeat(64),
            atMilliseconds: Date.now()
          };
        }
        if (kind === 'corrupt') record.outer = 'corrupt signed tail';
        if (kind === 'id') record.id = '0'.repeat(64);
        next = { ...original, wire: JSON.stringify(record) };
      }
      await put('received_envelopes', [next], other);
    },
    restoreTail: () => put('received_envelopes', [rows.at(-1)!]),
    async overfillReceived() {
      const secret = generateSecretKey(),
        conversation = nip44.v2.utils.getConversationKey(secret, owner);
      let event;
      try {
        event = finalizeEvent(
          {
            kind: 1059,
            created_at: Math.floor(Date.now() / 1000),
            tags: [['p', owner]],
            content: nip44.v2.encrypt(
              'CR005_TEST_ONLY_EXTRA_2001',
              conversation
            )
          },
          secret
        );
      } finally {
        secret.fill(0);
        conversation.fill(0);
      }
      const prior = JSON.parse(rows[0].wire) as Record<string, unknown>;
      const record = {
          ...prior,
          id: event.id,
          outer: JSON.stringify(event)
        },
        wire = JSON.stringify(record);
      if (!decodePrivateRecord(wire, owner, event.id).ok)
        throw Error('actual2001record invalid');
      await put('received_envelopes', [{ owner, id: event.id, wire }], true);
      const result = await listPrivateReceivedRecords(repo);
      return { ok: result.ok, reason: result.ok ? null : result.reason };
    },
    async overfillSends() {
      const extra = { ...sends[0], id: crypto.randomUUID() },
        record = JSON.parse(extra.wire) as Record<string, unknown>;
      record.id = extra.id;
      extra.wire = JSON.stringify(record);
      await put('private_sends', [extra], true);
      const result = await listPrivateSendRecords(repo);
      return { ok: result.ok, reason: result.ok ? null : result.reason };
    },
    async oversizeReceived() {
      const padded = rows.map((row) => {
          const record = JSON.parse(row.wire) as Record<string, unknown>;
          record.outer = String(record.outer) + '\n'.repeat(9000);
          return { ...row, wire: JSON.stringify(record) };
        }),
        bytes = padded.reduce(
          (n, row) => n + new TextEncoder().encode(JSON.stringify(row)).length,
          0
        );
      await put('received_envelopes', padded, true);
      const result = await listPrivateReceivedRecords(repo);
      return { bytes, ok: result.ok, reason: result.ok ? null : result.reason };
    },
    async foreignRead() {
      const result = await loadPrivateRecord(
        foreignRepo,
        'received_envelopes',
        rows[0].id
      );
      return { ok: result.ok, reason: result.ok ? null : result.reason };
    },
    close() {
      IDBIndex.prototype.openCursor = nativeCursor;
      disconnectIdentity(identity);
      closeBrowserDatabase(database);
      signer.close();
      peer.close();
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else delete window.nostr;
    }
  };
}
