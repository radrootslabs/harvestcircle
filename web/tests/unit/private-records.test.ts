import { describe, expect, it } from 'vitest';
import {
  generateSecretKey,
  getPublicKey,
  finalizeEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import {
  decodePrivateRecord,
  privateRecordSnapshot,
  privateRecordWire,
  inspectPrivateSendReservation
} from '../../src/lib/persistence/private-records.ts';
const command = '12345678-1234-4234-8234-123456789abc';
function fixture() {
  const key = generateSecretKey(),
    peerKey = generateSecretKey();
  let conversation: Uint8Array | undefined,
    peerConversation: Uint8Array | undefined;
  try {
    const owner = getPublicKey(key),
      peer = getPublicKey(peerKey);
    conversation = nip44.v2.utils.getConversationKey(key, owner);
    const outer = finalizeEvent(
      {
        kind: 1059,
        created_at: 1700000000,
        tags: [['p', owner]],
        content: nip44.v2.encrypt(
          'Ciphertext structure fixture only',
          conversation
        )
      },
      key
    );
    peerConversation = nip44.v2.utils.getConversationKey(key, peer);
    const peerOuter = finalizeEvent(
      {
        kind: 1059,
        created_at: 1700000001,
        tags: [['p', peer]],
        content: nip44.v2.encrypt(
          'Ciphertext structure fixture only',
          peerConversation
        )
      },
      key
    );
    const peerArtifact = {
      eventId: peerOuter.id,
      wire: JSON.stringify(peerOuter)
    };
    const reservation = {
      schema: 1,
      family: 'private_send_reservation',
      owner,
      id: command,
      revision: 0,
      peer,
      rumorHash: 'a'.repeat(64),
      createdAt: 1700000000
    };
    const operation = {
      ...reservation,
      family: 'private_send_operation',
      revision: 1,
      self: { eventId: outer.id, wire: JSON.stringify(outer) },
      peerArtifact: null
    };
    const received = {
      schema: 1,
      family: 'received_envelope',
      owner,
      id: outer.id,
      revision: 0,
      outer: JSON.stringify(outer),
      observedAtMilliseconds: 1700000000000,
      sources: ['wss://inbox.example.org'],
      read: null
    };
    return { owner, peer, reservation, operation, received, peerArtifact };
  } finally {
    conversation?.fill(0);
    peerConversation?.fill(0);
    key.fill(0);
    peerKey.fill(0);
  }
}
describe('ciphertext-only private local codecs, not nested authenticity or effect authority', () => {
  it('rejects duplicate or escaped-alias outer content fields instead of persisting hidden plaintext', () => {
    const f = fixture();
    for (const name of ['"content"', '"\\u0063ontent"']) {
      const poison = (raw: string) =>
        '{' + name + ':"PRIVATE_DUPLICATE_SENTINEL",' + raw.slice(1);
      const cases = [
        { ...f.received, outer: poison(f.received.outer) },
        {
          ...f.operation,
          self: { ...f.operation.self, wire: poison(f.operation.self.wire) }
        },
        {
          ...f.operation,
          peerArtifact: { ...f.peerArtifact, wire: poison(f.peerArtifact.wire) }
        }
      ];
      for (const record of cases)
        expect(
          decodePrivateRecord(JSON.stringify(record), f.owner, record.id).ok
        ).toBe(false);
    }
  });
  it('rejects duplicate local/nested metadata while retaining valid reordered/whitespace ciphertext bytes exactly', () => {
    const f = fixture();
    const duplicateRoot =
      '{"owner":"PRIVATE_DUPLICATE_SENTINEL",' +
      JSON.stringify(f.operation).slice(1);
    const duplicateArtifact = JSON.stringify(f.operation).replace(
      '"self":{',
      '"self":{"wire":"PRIVATE_DUPLICATE_SENTINEL",'
    );
    const read = {
      ...f.received,
      read: { rumorHash: 'b'.repeat(64), atMilliseconds: 1 }
    };
    const duplicateRead = JSON.stringify(read).replace(
      '"read":{',
      '"read":{"rumorHash":"PRIVATE_DUPLICATE_SENTINEL",'
    );
    for (const [raw, id] of [
      [duplicateRoot, f.operation.id],
      [duplicateArtifact, f.operation.id],
      [duplicateRead, f.received.id]
    ])
      expect(decodePrivateRecord(raw, f.owner, id).ok).toBe(false);
    const parsed = JSON.parse(f.received.outer) as Record<string, unknown>;
    const reordered =
      '\n' +
      JSON.stringify(
        {
          sig: parsed.sig,
          content: parsed.content,
          tags: parsed.tags,
          kind: parsed.kind,
          created_at: parsed.created_at,
          pubkey: parsed.pubkey,
          id: parsed.id
        },
        null,
        2
      ) +
      '\t';
    const record = { ...f.received, outer: reordered },
      decoded = decodePrivateRecord(JSON.stringify(record), f.owner, record.id);
    expect(decoded.ok).toBe(true);
    if (!decoded.ok) throw Error('valid whitespace/reordering rejected');
    expect(privateRecordSnapshot(decoded.value, f.owner, record.id)).toEqual(
      record
    );
  });
  it('admits strict metadata reservations, self recovery and received ciphertext separately from public records', () => {
    const f = fixture();
    for (const record of [f.reservation, f.operation, f.received]) {
      const result = decodePrivateRecord(
        JSON.stringify(record),
        f.owner,
        record.id
      );
      expect(result.ok).toBe(true);
      if (!result.ok) throw Error('expected bounded record');
      expect(privateRecordSnapshot(result.value, f.owner, record.id)).toEqual(
        record
      );
      expect(
        privateRecordSnapshot(result.value, f.peer, record.id)
      ).toBeUndefined();
      expect(privateRecordWire(result.value, f.owner, record.id)).toBe(
        JSON.stringify(record)
      );
      expect(
        privateRecordSnapshot({} as typeof result.value, f.owner, record.id)
      ).toBeUndefined();
    }
    expect(
      inspectPrivateSendReservation(
        JSON.stringify(f.reservation),
        f.owner,
        command
      )
    ).toEqual(f.reservation);
  });
  it('rejects plaintext fields, unknown schema/families, bad scope and unsafe metadata without coercion', () => {
    const f = fixture();
    for (const record of [f.reservation, f.operation, f.received]) {
      for (const key of ['body', 'subject', 'contact', 'product', 'content']) {
        expect(
          decodePrivateRecord(
            JSON.stringify({ ...record, [key]: 'private sentinel' }),
            f.owner,
            record.id
          ).ok
        ).toBe(false);
      }
      for (const patch of [
        { schema: 2 },
        { family: 'public_draft' },
        { revision: -1 },
        { revision: 1.5 },
        { owner: f.peer },
        { id: 'bad' }
      ]) {
        expect(
          decodePrivateRecord(
            JSON.stringify({ ...record, ...patch }),
            f.owner,
            record.id
          ).ok
        ).toBe(false);
      }
      expect(
        decodePrivateRecord(JSON.stringify(record), f.peer, record.id).ok
      ).toBe(false);
    }
    expect(
      decodePrivateRecord(
        { toString: () => JSON.stringify(f.operation) },
        f.owner,
        command
      ).ok
    ).toBe(false);
    expect(decodePrivateRecord('{bad', f.owner, command).ok).toBe(false);
  });
  it('requires exact signed bounded1059 ciphertext and role destination, never rumor or seal plaintext', () => {
    const f = fixture(),
      outer = JSON.parse(f.operation.self.wire) as Record<string, unknown>;
    for (const patch of [
      { kind: 14 },
      { kind: 13 },
      { content: 'cleartext' },
      { tags: [['p', f.peer]] },
      { sig: '0'.repeat(128) },
      { privateBody: 'sentinel' }
    ]) {
      const next = {
        ...f.operation,
        self: {
          eventId: f.operation.self.eventId,
          wire: JSON.stringify({ ...outer, ...patch })
        }
      };
      expect(
        decodePrivateRecord(JSON.stringify(next), f.owner, command).ok
      ).toBe(false);
    }
    const oversized = {
      ...f.operation,
      self: {
        ...f.operation.self,
        wire: f.operation.self.wire + ' '.repeat(32768)
      }
    };
    expect(
      decodePrivateRecord(JSON.stringify(oversized), f.owner, command).ok
    ).toBe(false);
    expect(
      decodePrivateRecord(
        JSON.stringify({ ...f.operation, self: null }),
        f.owner,
        command
      ).ok
    ).toBe(false);
    expect(
      decodePrivateRecord(
        JSON.stringify({ ...f.received, sources: ['https://inbox.example'] }),
        f.owner,
        f.received.id
      ).ok
    ).toBe(false);
    expect(
      decodePrivateRecord(
        JSON.stringify({
          ...f.received,
          read: { rumorHash: 'bad', atMilliseconds: 1 }
        }),
        f.owner,
        f.received.id
      ).ok
    ).toBe(false);
  });
});
