import { finalizeEvent } from 'applesauce-core/helpers';
import { validateRelayPolicy } from '../../../src/lib/config/relays.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../../src/lib/nostr/request-scope.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
export {
  reviewInboxSetup,
  inboxSetupPreview,
  captureInboxSetup
} from '../../../src/lib/messaging/inbox-setup.ts';
export { saveInboxPreferenceOperation } from '../../../src/lib/persistence/inbox-preference-operations.ts';
export {
  openBrowserDatabase,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
export {
  createPublicQuotaRepository,
  inspectPublicStorage,
  publicInventorySnapshot
} from '../../../src/lib/persistence/quota.ts';
export {
  publicRecordSnapshot,
  publicRecordWire
} from '../../../src/lib/persistence/records.ts';
export const discovery = 'wss://discovery.example.org',
  inbox = 'wss://inbox.example.org';
export function fixturePolicy() {
  return validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [{ origin: discovery, read: true, write: true, nip50: false }],
      inbox: [{ origin: inbox, read: true, write: true }],
      postingEnabled: true,
      messagingEnabled: true,
      operatorDenylist: []
    })
  )!;
}
// Actual SDK identity/discovery ownership with controlled transport/echo provider;
// source fixture only, never a real extension, inbox or retention qualification.
export async function makeSetupFixture(
  tags: string[][] = [
    ['relay', 'wss://old.example.org'],
    ['unknown', 'keep'],
    ['unknown', 'keep']
  ],
  extra: Record<string, unknown> = {},
  missing = false
) {
  const key = crypto.getRandomValues(new Uint8Array(32));
  const event = finalizeEvent(
    { kind: 10050, created_at: 100, tags, content: 'existing public content' },
    key
  );
  const newerEvent = finalizeEvent(
    { kind: 10050, created_at: 101, tags, content: 'concurrent preference' },
    key
  );
  key.fill(0);
  const identity = createIdentitySession(),
    prior = Object.getOwnPropertyDescriptor(globalThis, 'window');
  const made = typeof window === 'undefined';
  if (made)
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: {}
    });
  const priorProvider = Object.getOwnPropertyDescriptor(window, 'nostr');
  let signs = 0,
    keys = 0;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () => {
        keys++;
        return Promise.resolve(event.pubkey);
      },
      signEvent: () => {
        signs++;
        return Promise.reject(new Error('unexpected preference signature'));
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
  const schedulers: ReturnType<typeof createPublicScheduler>[] = [];
  function resolve(
    currentEvent: typeof event | null = missing ? null : { ...event, ...extra },
    complete = true
  ) {
    let current = true;
    const scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    });
    schedulers.push(scheduler);
    const run = createPublicRun(scheduler, fixturePolicy());
    const resolver = createInboxResolver(
      run,
      event.pubkey,
      (next) =>
        openInboxRequest(
          run,
          event.pubkey,
          (sink) => {
            if (currentEvent !== null)
              sink({
                type: 'EVENT',
                from: discovery,
                id: 'fixture',
                event: currentEvent
              });
            if (complete)
              sink({ type: 'EOSE', from: discovery, id: 'fixture' });
            return () => {};
          },
          next
        ),
      () => current
    );
    return {
      resolver,
      stale: () => {
        current = false;
      }
    };
  }
  const own = resolve();
  return {
    identity,
    owner: event.pubkey,
    event,
    newerEvent,
    own,
    resolve,
    counts: () => ({ signs, keys }),
    close() {
      disconnectIdentity(identity);
      for (const s of schedulers) closePublicScheduler(s);
      if (priorProvider) Object.defineProperty(window, 'nostr', priorProvider);
      else Reflect.deleteProperty(window, 'nostr');
      if (made) {
        if (prior) Object.defineProperty(globalThis, 'window', prior);
        else Reflect.deleteProperty(globalThis, 'window');
      }
    }
  };
}
export function setupInput(overrides: Record<string, unknown> = {}) {
  return JSON.stringify({
    selectedInboxes: [inbox],
    removeTagIndices: [],
    removeExtraFields: [],
    createdAt: 101,
    ...overrides
  });
}
