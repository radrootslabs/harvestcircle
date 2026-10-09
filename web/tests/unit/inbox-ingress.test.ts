import { describe, it, expect } from 'vitest';
import {
  generateSecretKey,
  getPublicKey,
  finalizeEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import {
  admitInboxEnvelope,
  inboxEnvelopeSnapshot
} from '../../src/lib/persistence/inbox-envelope-repository.ts';
import {
  inboxIngressSnapshot,
  type InboxIngress
} from '../../src/lib/nostr/inbox-ingress.ts';
function fixture(kind = 1059, target?: string, content?: string) {
  const key = generateSecretKey(),
    other = generateSecretKey();
  try {
    const owner = getPublicKey(other),
      destination = target ?? owner,
      conversation = nip44.v2.utils.getConversationKey(key, destination);
    try {
      const event = finalizeEvent(
        {
          kind,
          created_at: 1700000000,
          tags: [['p', destination]],
          content:
            content ??
            nip44.v2.encrypt(
              'Encrypted structural outer fixture only',
              conversation
            )
        },
        key
      );
      return { owner, event, wire: JSON.stringify(event) };
    } finally {
      conversation.fill(0);
    }
  } finally {
    key.fill(0);
    other.fill(0);
  }
}
describe('bounded untrusted owner-addressed inbox outer', () => {
  it('admits actual signed1059 as ciphertext only, not a trusted nested message', () => {
    const f = fixture(),
      token = admitInboxEnvelope(
        f.wire,
        f.owner,
        'wss://archive.example.org',
        100
      );
    expect(token).toBeDefined();
    const view = inboxEnvelopeSnapshot(token!);
    expect(view).toMatchObject({
      owner: f.owner,
      id: f.event.id,
      outer: f.wire,
      read: null
    });
    expect(view).not.toHaveProperty('body');
    expect(view).not.toHaveProperty('sender');
  });
  it('wrong destination cannot enter this owner namespace', () => {
    const f = fixture();
    expect(
      admitInboxEnvelope(
        f.wire,
        'b'.repeat(64),
        'wss://archive.example.org',
        100
      )
    ).toBeUndefined();
  });
  it('bad signature is rejected before retention', () => {
    const f = fixture();
    expect(
      admitInboxEnvelope(
        JSON.stringify({ ...f.event, sig: '0'.repeat(128) }),
        f.owner,
        'wss://archive.example.org',
        100
      )
    ).toBeUndefined();
  });
  it('non1059 never becomes a private inbox outer', () => {
    const f = fixture(14);
    expect(
      admitInboxEnvelope(f.wire, f.owner, 'wss://archive.example.org', 100)
    ).toBeUndefined();
  });
  it('unsupported outer ciphertext version is isolated despite valid outer signature', () => {
    const f = fixture(
      1059,
      undefined,
      btoa(String.fromCharCode(1) + 'a'.repeat(100))
    );
    expect(
      admitInboxEnvelope(f.wire, f.owner, 'wss://archive.example.org', 100)
    ).toBeUndefined();
  });
  it('32KiB outer bound is enforced before retained credit', () => {
    const f = fixture(
      1059,
      undefined,
      btoa(String.fromCharCode(2) + 'a'.repeat(30000))
    );
    expect(
      admitInboxEnvelope(f.wire, f.owner, 'wss://archive.example.org', 100)
    ).toBeUndefined();
  });
  it('malformed scope and observer metadata do not coerce caller values', () => {
    const f = fixture();
    let calls = 0;
    const object = {
      toString() {
        calls++;
        return f.wire;
      }
    };
    expect(
      admitInboxEnvelope(object, f.owner, 'wss://archive.example.org', 100)
    ).toBeUndefined();
    expect(
      admitInboxEnvelope(f.wire, f.owner, 'ws://127.0.0.1', 100)
    ).toBeUndefined();
    expect(
      admitInboxEnvelope(f.wire, f.owner, 'wss://archive.example.org', -1)
    ).toBeUndefined();
    expect(calls).toBe(0);
  });
  it('forged ingress handle cannot acquire a private receive workflow', () => {
    expect(inboxIngressSnapshot({} as InboxIngress)).toBeUndefined();
  });
});
