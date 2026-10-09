import { describe, it, expect } from 'vitest';
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
  type PrivateRecordHandle
} from '../../src/lib/persistence/private-records.ts';
import {
  preparePrivateReceiptTransition,
  privateReceiptTransitionSnapshot
} from '../../src/lib/persistence/private-receipts.ts';
import { privateSendStatus } from '../../src/lib/messaging/send-status.ts';
const command = '12345678-1234-4234-8234-123456789abc',
  action = '12345678-1234-4234-8234-123456789abd';
function fixture() {
  const key = generateSecretKey(),
    other = generateSecretKey();
  let conversation: Uint8Array | undefined;
  try {
    const owner = getPublicKey(key),
      peer = getPublicKey(other);
    function artifact(destination: string) {
      conversation = nip44.v2.utils.getConversationKey(key, destination);
      try {
        const event = finalizeEvent(
          {
            kind: 1059,
            created_at: 1700000000,
            tags: [['p', destination]],
            content: nip44.v2.encrypt(
              'Structural codec fixture only',
              conversation
            )
          },
          key
        );
        return { eventId: event.id, wire: JSON.stringify(event) };
      } finally {
        conversation.fill(0);
      }
    }
    function route(
      author: string,
      role: 'peer' | 'self_archive',
      origin: string
    ) {
      return {
        author,
        role,
        targets: [origin],
        knownBase: { author, id: 'b'.repeat(64), createdAt: 1 },
        sources: [{ source: 'wss://discovery.example.org', state: 'eose' }]
      };
    }
    const row = {
      schema: 1,
      family: 'private_send_operation',
      owner,
      id: command,
      revision: 2,
      peer,
      rumorHash: 'a'.repeat(64),
      createdAt: 1700000000,
      self: artifact(owner),
      peerArtifact: artifact(peer),
      deliveryPlan: {
        state: 'prepared',
        routes: {
          peer: route(peer, 'peer', 'wss://peer.example.org'),
          archive: route(owner, 'self_archive', 'wss://archive.example.org')
        }
      }
    };
    function handle(value: unknown = row) {
      const result = decodePrivateRecord(JSON.stringify(value), owner, command);
      if (!result.ok) throw Error('invalid structural fixture');
      return result.value;
    }
    function fact(role: 'peer' | 'self_archive' = 'peer', status = 'accepted') {
      return {
        actionId: action,
        role,
        origin:
          role === 'peer'
            ? 'wss://peer.example.org'
            : 'wss://archive.example.org',
        attempt: 1,
        eventId: role === 'peer' ? row.peerArtifact.eventId : row.self.eventId,
        status,
        observedAtMilliseconds: 100,
        readbackWire: null
      };
    }
    function transition(base: PrivateRecordHandle, f: unknown) {
      return preparePrivateReceiptTransition(
        base,
        owner,
        command,
        JSON.stringify(f)
      );
    }
    function next(base: PrivateRecordHandle, f: unknown) {
      const result = transition(base, f);
      if (!result.ok) throw Error('transition refused');
      const saved = privateReceiptTransitionSnapshot(
        result.value,
        owner,
        command
      );
      if (!saved) throw Error('missing transition');
      return saved.next;
    }
    return { row, owner, handle, fact, transition, next };
  } finally {
    conversation?.fill(0);
    key.fill(0);
    other.fill(0);
  }
}
describe('private named relay observations are separate local evidence', () => {
  it('legacy paired ciphertext without receipt metadata stays readable and uncertain', () => {
    const f = fixture(),
      h = f.handle();
    expect(privateRecordSnapshot(h, f.owner, command)).toEqual(f.row);
    const s = privateSendStatus(h, f.owner, command)!;
    expect(s.recipient.state).toBe('unknown');
    expect(s.archive.state).toBe('pending');
  });
  it('forged record and transition handles cannot supply status or receipt custody', () => {
    expect(
      privateSendStatus({} as PrivateRecordHandle, 'a'.repeat(64), command)
    ).toBeUndefined();
    expect(
      preparePrivateReceiptTransition(
        {} as PrivateRecordHandle,
        'a'.repeat(64),
        command,
        '{}'
      ).ok
    ).toBe(false);
  });
  it('archive acceptance alone never means recipient acceptance', () => {
    const f = fixture(),
      h = f.next(f.handle(), f.fact('self_archive'));
    const s = privateSendStatus(h, f.owner, command)!;
    expect(s.recipient.state).toBe('unknown');
    expect(s.archive.state).toBe('accepted_by_inbox_relay');
    expect(s.labels).toContain('Recipient delivery unknown');
    expect(s.partial).toBe(true);
  });
  it('recipient acceptance preserves the separate sender archive pending fact', () => {
    const f = fixture(),
      h = f.next(f.handle(), f.fact());
    const s = privateSendStatus(h, f.owner, command)!;
    expect(s.recipient.state).toBe('accepted_by_inbox_relay');
    expect(s.archive.state).toBe('pending');
    expect(s.labels).toContain('Sender archive pending');
  });
  it('late duplicate ACK is idempotent even when sampled later', () => {
    const f = fixture(),
      a = f.fact(),
      base = f.next(f.handle(), a),
      again = f.next(base, { ...a, observedAtMilliseconds: 200 });
    expect(privateRecordWire(again, f.owner, command)).toBe(
      privateRecordWire(base, f.owner, command)
    );
  });
  it('distinct refusal and later acceptance observations are both retained', () => {
    const f = fixture(),
      a = f.next(f.handle(), f.fact('peer', 'refused')),
      b = f.next(a, f.fact());
    const row = privateRecordSnapshot(b, f.owner, command);
    expect(
      row?.family === 'private_send_operation' &&
        row.receipts?.map((r) => r.status)
    ).toEqual(['refused', 'accepted']);
    expect(privateSendStatus(b, f.owner, command)?.recipient.state).toBe(
      'accepted_by_inbox_relay'
    );
  });
  it('exact peer ciphertext readback is separate from a relay ACK', () => {
    const f = fixture(),
      fact = {
        ...f.fact('peer', 'readback'),
        readbackWire: f.row.peerArtifact.wire
      },
      h = f.next(f.handle(), fact),
      s = privateSendStatus(h, f.owner, command)!;
    expect(s.recipient.readbackTargets).toEqual(['wss://peer.example.org']);
    expect(s.recipient.acceptedTargets).toEqual([]);
    expect(s.recipient.state).toBe('unknown');
  });
  it('archive ciphertext cannot serve as peer readback', () => {
    const f = fixture();
    expect(
      f.transition(f.handle(), {
        ...f.fact('peer', 'readback'),
        readbackWire: f.row.self.wire
      }).ok
    ).toBe(false);
  });
  it('wrong role destination event and unsupported plaintext fields refuse', () => {
    const f = fixture(),
      base = f.fact();
    for (const bad of [
      { ...base, role: 'publication' },
      { ...base, origin: 'wss://other.example.org' },
      { ...base, eventId: f.row.self.eventId },
      { ...base, body: 'PRIVATE_TEXT' },
      { ...base, readbackWire: f.row.peerArtifact.wire }
    ])
      expect(f.transition(f.handle(), bad).ok).toBe(false);
  });
  it('unsafe times attempts and arbitrary outcome strings refuse', () => {
    const f = fixture(),
      base = f.fact();
    for (const bad of [
      { ...base, attempt: 0 },
      { ...base, attempt: 4 },
      { ...base, observedAtMilliseconds: -1 },
      { ...base, observedAtMilliseconds: Number.MAX_SAFE_INTEGER + 1 },
      { ...base, status: 'delivered' }
    ])
      expect(f.transition(f.handle(), bad).ok).toBe(false);
  });
  it('unknown stopped timeout and refusal never imply recipient success', () => {
    const f = fixture();
    for (const status of ['unknown', 'stopped', 'timed_out', 'refused'])
      expect(
        privateSendStatus(
          f.next(f.handle(), f.fact('peer', status)),
          f.owner,
          command
        )?.recipient.state
      ).toBe('unknown');
  });
  it('receipt transitions preserve exact original ciphertext and frozen routes', () => {
    const f = fixture(),
      base = f.handle(),
      next = f.next(base, f.fact()),
      row = privateRecordSnapshot(next, f.owner, command);
    expect(row?.family === 'private_send_operation' && row.self).toEqual(
      f.row.self
    );
    expect(
      row?.family === 'private_send_operation' && row.peerArtifact
    ).toEqual(f.row.peerArtifact);
    expect(
      row?.family === 'private_send_operation' && row.deliveryPlan
    ).toEqual(f.row.deliveryPlan);
    expect(privateRecordWire(base, f.owner, command)).toBe(
      JSON.stringify(f.row)
    );
  });
  it('revision exhaustion refuses a new fact', () => {
    const f = fixture();
    expect(
      f.transition(
        f.handle({ ...f.row, revision: Number.MAX_SAFE_INTEGER }),
        f.fact()
      ).ok
    ).toBe(false);
  });
  it('receipt acquisition never coerces caller objects or noncanonical duplicate JSON', () => {
    const f = fixture();
    let calls = 0;
    const input = {
      toString() {
        calls++;
        return JSON.stringify(f.fact());
      }
    };
    expect(
      preparePrivateReceiptTransition(f.handle(), f.owner, command, input).ok
    ).toBe(false);
    expect(calls).toBe(0);
    const raw = JSON.stringify(f.fact());
    expect(
      preparePrivateReceiptTransition(
        f.handle(),
        f.owner,
        command,
        '{"status":"accepted",' + raw.slice(1)
      ).ok
    ).toBe(false);
  });
});
