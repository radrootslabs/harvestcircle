import { expect, it } from 'vitest';
import { finalizeEvent } from 'applesauce-core/helpers';
import {
  readInboxPreference,
  inboxPreferenceSnapshot,
  inboxPreferenceWire,
  type InboxPreference
} from '../../src/lib/nostr/inbox-preferences.ts';
import { inboxProfileSnapshot } from '../../src/lib/messaging/inbox-profile.ts';

it('forged preference handles grant no profile, wire, provider or SSR authority', () => {
  expect(typeof window).toBe('undefined');
  const forged = {} as InboxPreference;
  expect(inboxPreferenceSnapshot(forged)).toBeUndefined();
  expect(inboxPreferenceWire(forged)).toBeUndefined();
  expect(inboxProfileSnapshot(forged)).toBeUndefined();
  expect(readInboxPreference({}, 'not-a-key').status).toBe('rejected');
});
it('detached profile mutation never changes proof-owned original or advertised entries', () => {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const event = finalizeEvent(
      {
        kind: 10050,
        created_at: 100,
        tags: [
          ['relay', 'wss://one.example.org'],
          ['unknown', 'keep']
        ],
        content: ''
      },
      key
    );
    const wire = JSON.stringify(event);
    const parsed = readInboxPreference(wire, event.pubkey);
    expect(parsed.status).toBe('supported');
    if (parsed.status !== 'supported') throw new Error('fixture not admitted');
    const view = inboxProfileSnapshot(parsed.value)!;
    view.advertised.length = 0;
    expect(inboxProfileSnapshot(parsed.value)?.advertised).toEqual([
      'wss://one.example.org'
    ]);
    expect(inboxPreferenceWire(parsed.value)).toBe(wire);
    expect(Object.isFrozen(parsed.value)).toBe(true);
  } finally {
    key.fill(0);
  }
});
