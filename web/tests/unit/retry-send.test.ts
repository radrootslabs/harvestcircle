import { describe, it, expect } from 'vitest';
import {
  generateSecretKey,
  getPublicKey,
  finalizeEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import {
  decodePrivateRecord,
  type PrivateRecordHandle
} from '../../src/lib/persistence/private-records.ts';
import {
  privateRetryTargets,
  runPrivateRetry,
  type PrivateRetry
} from '../../src/lib/messaging/retry-send.ts';
function fixture(status?: string, role = 'peer') {
  const key = generateSecretKey(),
    other = generateSecretKey();
  try {
    const owner = getPublicKey(key),
      peer = getPublicKey(other),
      id = '12345678-1234-4234-8234-123456789abc';
    function artifact(destination: string) {
      const conversation = nip44.v2.utils.getConversationKey(key, destination);
      try {
        const event = finalizeEvent(
          {
            kind: 1059,
            created_at: 1700000000,
            tags: [['p', destination]],
            content: nip44.v2.encrypt('Structural fixture only', conversation)
          },
          key
        );
        return { eventId: event.id, wire: JSON.stringify(event) };
      } finally {
        conversation.fill(0);
      }
    }
    function route(author: string, role: string, origin: string) {
      return {
        author,
        role,
        targets: [origin],
        knownBase: { author, id: 'b'.repeat(64), createdAt: 1 },
        sources: [{ source: 'wss://discovery.example.org', state: 'eose' }]
      };
    }
    const self = artifact(owner),
      peerArtifact = artifact(peer);
    const row = {
      schema: 1,
      family: 'private_send_operation',
      owner,
      id,
      revision: 2,
      peer,
      rumorHash: 'a'.repeat(64),
      createdAt: 1700000000,
      self,
      peerArtifact,
      deliveryPlan: {
        state: 'prepared',
        routes: {
          peer: route(peer, 'peer', 'wss://peer.example.org'),
          archive: route(owner, 'self_archive', 'wss://archive.example.org')
        }
      },
      ...(status
        ? {
            receipts: [
              {
                actionId: '12345678-1234-4234-8234-123456789abd',
                role,
                origin:
                  role === 'peer'
                    ? 'wss://peer.example.org'
                    : 'wss://archive.example.org',
                attempt: 1,
                eventId: role === 'peer' ? peerArtifact.eventId : self.eventId,
                status,
                observedAtMilliseconds: 100,
                readbackWire: status === 'readback' ? peerArtifact.wire : null
              }
            ]
          }
        : {})
    };
    const decoded = decodePrivateRecord(JSON.stringify(row), owner, id);
    if (!decoded.ok) throw Error('invalid structural fixture');
    return { owner, id, handle: decoded.value, row };
  } finally {
    key.fill(0);
    other.fill(0);
  }
}
describe('explicit exact-artifact retry target selection', () => {
  it('retains the exact stored role event IDs and target order', () => {
    const f = fixture();
    expect(privateRetryTargets(f.handle, f.owner, f.id)).toEqual([
      {
        role: 'peer',
        origin: 'wss://peer.example.org',
        eventId: f.row.peerArtifact.eventId
      },
      {
        role: 'self_archive',
        origin: 'wss://archive.example.org',
        eventId: f.row.self.eventId
      }
    ]);
  });
  it('skips only the exact accepted role and preserves the other artifact target', () => {
    for (const role of ['peer', 'self_archive']) {
      const f = fixture('accepted', role),
        targets = privateRetryTargets(f.handle, f.owner, f.id)!;
      expect(targets).toHaveLength(1);
      expect(targets[0].role).toBe(role === 'peer' ? 'self_archive' : 'peer');
    }
  });
  it('codec-only readback does not impersonate authenticated remote retrieval or ACK', () => {
    const f = fixture('readback');
    expect(privateRetryTargets(f.handle, f.owner, f.id)).toHaveLength(2);
  });
  it('unknown timeout refusal and stopped facts do not supply success', () => {
    for (const status of ['unknown', 'timed_out', 'refused', 'stopped']) {
      const f = fixture(status);
      expect(privateRetryTargets(f.handle, f.owner, f.id)).toHaveLength(2);
    }
  });
  it('forged handles and a foreign owner cannot select private targets', () => {
    const f = fixture();
    expect(
      privateRetryTargets({} as PrivateRecordHandle, f.owner, f.id)
    ).toBeUndefined();
    expect(privateRetryTargets(f.handle, 'b'.repeat(64), f.id)).toBeUndefined();
  });
  it('an arbitrary object cannot start a retry or cause an implicit action', async () => {
    expect(
      await runPrivateRetry({} as PrivateRetry, 'reviewed_private_retry')
    ).toEqual({ status: 'invalid' });
  });
});
