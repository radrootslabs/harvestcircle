import { makeFixture } from './approved-signing.ts';
import { Relay } from 'applesauce-relay/relay';
export {
  getPublicPool,
  closePublicPool
} from '../../../src/lib/nostr/public-pool.ts';
import { publicRecordSnapshot } from '../../../src/lib/persistence/records.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../../src/lib/nostr/request-scope.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import { fixturePolicy, discovery, setupInput } from './inbox-setup.ts';
export { fixturePolicy, discovery, setupInput };
export {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
};
export {
  reviewInboxSetup,
  inboxSetupPreview
} from '../../../src/lib/messaging/inbox-setup.ts';
export { saveInboxPreferenceOperation } from '../../../src/lib/persistence/inbox-preference-operations.ts';
export {
  beginInboxPreferenceOperation,
  resumeInboxPreferenceOperation,
  runInboxPreferenceOperation,
  stopInboxPreferenceOperation
} from '../../../src/lib/messaging/inbox-setup-operation.ts';
export {
  openBrowserDatabase,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
export {
  createPublicQuotaRepository,
  loadPreferenceOperation
} from '../../../src/lib/persistence/quota.ts';
export { publicRecordSnapshot };
export async function makePublisherFixture() {
  const fixture = makeFixture(10050),
    original = publicRecordSnapshot(fixture.record, fixture.owner, fixture.id)!;
  if (original.family !== 'preference_operation' || !original.source.wire)
    throw new Error('missing source');
  const source = JSON.parse(original.source.wire) as ReturnType<
    typeof fixture.sign
  >;
  const identity = createIdentitySession();
  let observedOwner = fixture.owner,
    signs = 0;
  let beforeSign: (() => Promise<void>) | undefined;
  const prior = Object.getOwnPropertyDescriptor(window, 'nostr');
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () => Promise.resolve(observedOwner),
      signEvent: async (input: Parameters<typeof fixture.sign>[0]) => {
        signs++;
        await beforeSign?.();
        return fixture.sign(input);
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
  const policy = fixturePolicy(),
    schedulers: ReturnType<typeof createPublicScheduler>[] = [];
  function resolve(event = source, complete = true) {
    const scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    });
    schedulers.push(scheduler);
    const run = createPublicRun(scheduler, policy);
    return createInboxResolver(
      run,
      fixture.owner,
      (next) =>
        openInboxRequest(
          run,
          fixture.owner,
          (sink) => {
            sink({ type: 'EVENT', from: discovery, id: 'fixture', event });
            if (complete)
              sink({ type: 'EOSE', from: discovery, id: 'fixture' });
            return () => {};
          },
          next
        ),
      () => true
    );
  }
  return {
    identity,
    policy,
    owner: fixture.owner,
    resolve,
    signs: () => signs,
    beforeSign(callback: () => Promise<void>) {
      beforeSign = callback;
    },
    changeOwner() {
      const other = makeFixture(10050);
      try {
        observedOwner = other.owner;
      } finally {
        other.close();
      }
    },
    competing() {
      return fixture.sign({
        ...fixture.template,
        created_at: 102,
        content: 'concurrent preference'
      });
    },
    close() {
      disconnectIdentity(identity);
      for (const scheduler of schedulers) closePublicScheduler(scheduler);
      fixture.close();
      if (prior) Object.defineProperty(window, 'nostr', prior);
      else Reflect.deleteProperty(window, 'nostr');
    }
  };
}
// Test-only fault at the real IDB write boundary; no mock database/production port.
export function failPreferenceWrite(revision: number) {
  const original = Reflect.get<IDBObjectStore, 'put'>(
    IDBObjectStore.prototype,
    'put'
  );
  IDBObjectStore.prototype.put = function (value: unknown, key?: IDBValidKey) {
    if (
      this.name === 'preference_operations' &&
      typeof value === 'object' &&
      value &&
      'wire' in value &&
      typeof value.wire === 'string'
    ) {
      const row = JSON.parse(value.wire) as { revision: number };
      if (row.revision === revision) {
        IDBObjectStore.prototype.put = original;
        this.transaction.abort();
        throw new Error('controlled IDB abort');
      }
    }
    return original.call(this, value, key);
  };
  return () => {
    IDBObjectStore.prototype.put = original;
  };
}
// Actual transaction-complete and native SDK transport-send ordering only.
// Delegates both APIs unchanged; no delayed send or database substitute.
export function installEffectTrace() {
  const events: string[] = [];
  const put = Reflect.get<IDBObjectStore, 'put'>(
    IDBObjectStore.prototype,
    'put'
  );
  const send = Reflect.get<WebSocket, 'send'>(WebSocket.prototype, 'send');
  IDBObjectStore.prototype.put = function (value: unknown, key?: IDBValidKey) {
    if (
      this.name === 'preference_operations' &&
      typeof value === 'object' &&
      value &&
      'wire' in value &&
      typeof value.wire === 'string'
    ) {
      const revision = (JSON.parse(value.wire) as { revision: number })
        .revision;
      this.transaction.addEventListener(
        'complete',
        () => events.push('ack:' + revision),
        { once: true }
      );
    }
    return put.call(this, value, key);
  };
  WebSocket.prototype.send = function (data: Parameters<WebSocket['send']>[0]) {
    if (
      typeof data === 'string' &&
      (JSON.parse(data) as unknown[])[0] === 'EVENT'
    )
      events.push('EVENT');
    return send.call(this, data);
  };
  return {
    events: () => [...events],
    restore() {
      IDBObjectStore.prototype.put = put;
      WebSocket.prototype.send = send;
    }
  };
}
// Retain the actual SDK observable/subscription; only its owned release is
// faulted. No fake ACK, transport, signature, database or production option.
export function installUnsubscribeFailure() {
  const event = Reflect.get<Relay, 'event'>(Relay.prototype, 'event');
  let fail = true;
  Relay.prototype.event = function (...args: Parameters<Relay['event']>) {
    const observable = event.call(this, ...args);
    observable.subscribe = new Proxy(observable.subscribe.bind(observable), {
      apply(subscribe, receiver, argumentsList) {
        const subscription = Reflect.apply(
          subscribe,
          receiver,
          argumentsList
        ) as ReturnType<typeof subscribe>;
        const release = subscription.unsubscribe.bind(subscription);
        subscription.unsubscribe = () => {
          if (fail) throw new Error('controlled owned release failure');
          release();
        };
        return subscription;
      }
    });
    return observable;
  };
  return {
    allowRelease() {
      fail = false;
    },
    restore() {
      fail = false;
      Relay.prototype.event = event;
    }
  };
}
