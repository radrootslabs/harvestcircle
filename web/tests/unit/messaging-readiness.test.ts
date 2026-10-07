import { it, expect } from 'vitest';
import { finalizeEvent } from 'applesauce-core/helpers';
import {
  validateRelayPolicy,
  type RelayPolicy
} from '../../src/lib/config/relays.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../src/lib/nostr/request-scope.ts';
import {
  createInboxResolver,
  type InboxResolver
} from '../../src/lib/messaging/resolve-inbox.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity,
  identitySessionSnapshot
} from '../../src/lib/runtime/identity-session.ts';
import {
  createPrivateSession,
  closePrivateSession
} from '../../src/lib/runtime/private-session.ts';
import {
  messagingReadinessSnapshot,
  type OwnInboxAccessContext,
  type InboxAccessObservation
} from '../../src/lib/messaging/readiness.ts';
const discovery = 'wss://discovery.example.org',
  ownOrigin = 'wss://archive.example.org',
  peerOrigin = 'wss://peer.example.org';
function policy(): RelayPolicy {
  return validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [{ origin: discovery, read: true, write: false, nip50: false }],
      inbox: [ownOrigin, peerOrigin].map((origin) => ({
        origin,
        read: true,
        write: true
      })),
      postingEnabled: true,
      messagingEnabled: true,
      operatorDenylist: []
    })
  )!;
}
function signed(origin: string) {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    return finalizeEvent(
      { kind: 10050, created_at: 100, tags: [['relay', origin]], content: '' },
      key
    );
  } finally {
    key.fill(0);
  }
}
function resolve(event: ReturnType<typeof signed>, complete = true) {
  const scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    }),
    run = createPublicRun(scheduler, policy());
  let current = true;
  const resolver = createInboxResolver(
    run,
    event.pubkey,
    (next) =>
      openInboxRequest(
        run,
        event.pubkey,
        (sink) => {
          sink({ type: 'EVENT', from: discovery, id: 'fixture', event });
          if (complete) sink({ type: 'EOSE', from: discovery, id: 'fixture' });
          return () => {};
        },
        next
      ),
    () => current
  );
  return {
    resolver,
    close() {
      closePublicScheduler(scheduler);
    },
    stale() {
      current = false;
    }
  };
}
// HC_TEST_ONLY_QUALIFICATION_PORT: controlled observations test the projection;
// neither this echo provider nor its claims qualify a real extension or relay.
function qualified(c: OwnInboxAccessContext): InboxAccessObservation {
  return {
    owner: c.owner,
    session: c.session,
    preferenceId: c.preferenceId,
    readTargets: c.readTargets.slice(),
    archiveTargets: c.archiveTargets.slice(),
    receive: 'qualified_exercised',
    archive: 'qualified_exercised',
    current: () => true
  };
}
async function fixture(messaging = true) {
  const self = signed(ownOrigin),
    peer = signed(peerOrigin),
    own = resolve(self),
    other = resolve(peer),
    identity = createIdentitySession();
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'window');
  let observations = 0,
    signatures = 0;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      nostr: {
        getPublicKey() {
          observations++;
          return Promise.resolve(self.pubkey);
        },
        signEvent() {
          signatures++;
          return Promise.reject(new Error('HC_TEST_ONLY_UNEXPECTED_SIGN'));
        },
        ...(messaging
          ? {
              nip44: {
                encrypt: (_peer: string, text: string) =>
                  Promise.resolve('fixture:' + text),
                decrypt: (_peer: string, text: string) =>
                  Promise.resolve(text.slice(8))
              }
            }
          : {})
      }
    }
  });
  await connectIdentity(identity);
  if (messaging) await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const session = await createPrivateSession(
    identity,
    'reviewed_private_session'
  );
  return {
    self,
    peer,
    own,
    other,
    identity,
    session,
    counts: () => ({ observations, signatures }),
    close() {
      if (session) closePrivateSession(session);
      disconnectIdentity(identity);
      own.close();
      other.close();
      if (previous) Object.defineProperty(globalThis, 'window', previous);
      else Reflect.deleteProperty(globalThis, 'window');
    }
  };
}
it('SSR and guest browse/draft do not need inbox setup or identity effects', () => {
  const r = messagingReadinessSnapshot(
    createIdentitySession(),
    undefined,
    policy(),
    undefined
  );
  expect(r).toMatchObject({
    browse: true,
    draft: true,
    newListingReady: false,
    sendReady: false,
    preferenceWrite: 'not_requested'
  });
});
it('signing-only real SDK observation cannot advertise a messaging-ready new listing', async () => {
  const f = await fixture(false);
  try {
    expect(f.session).toBeUndefined();
    const before = f.counts();
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        f.own.resolver,
        { author: f.peer.pubkey, resolver: f.other.resolver },
        qualified
      )
    ).toMatchObject({ newListingReady: false, sendReady: false });
    expect(f.counts()).toEqual(before);
  } finally {
    f.close();
  }
});
it('compatible existing own preference needs no write and qualified source fixture permits prepared exchange', async () => {
  const f = await fixture();
  try {
    const before = f.counts();
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        f.own.resolver,
        { author: f.peer.pubkey, resolver: f.other.resolver },
        qualified
      )
    ).toMatchObject({
      ownConfiguration: 'compatible',
      ownAccess: 'qualified_exercised',
      newListingReady: true,
      sendReady: true,
      preferenceWrite: 'not_requested'
    });
    expect(f.counts()).toEqual(before);
  } finally {
    f.close();
  }
});
it('capable connection and compatible routes without qualified access stay blocked', async () => {
  const f = await fixture();
  try {
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        f.own.resolver,
        { author: f.peer.pubkey, resolver: f.other.resolver }
      )
    ).toMatchObject({
      ownConfiguration: 'compatible',
      ownAccess: 'unavailable',
      newListingReady: false,
      sendReady: false
    });
  } finally {
    f.close();
  }
});
it('unknown peer cannot enable send while qualified own configuration remains useful', async () => {
  const f = await fixture(),
    partial = resolve(f.peer, false);
  try {
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        f.own.resolver,
        { author: f.peer.pubkey, resolver: partial.resolver },
        qualified
      )
    ).toMatchObject({ newListingReady: true, sendReady: false });
  } finally {
    partial.close();
    f.close();
  }
});
it('qualified access observation must match actual account generation preference and fixed targets', async () => {
  const f = await fixture();
  try {
    for (const mutate of [
      (r: InboxAccessObservation) => ({ ...r, owner: f.peer.pubkey }),
      (r: InboxAccessObservation) => ({ ...r, session: Symbol() }),
      (r: InboxAccessObservation) => ({ ...r, preferenceId: '0'.repeat(64) }),
      (r: InboxAccessObservation) => ({
        ...r,
        readTargets: ['wss://hint.example.org']
      }),
      (r: InboxAccessObservation) => ({ ...r, archiveTargets: [] })
    ]) {
      expect(
        messagingReadinessSnapshot(
          f.identity,
          f.session,
          policy(),
          f.own.resolver,
          { author: f.peer.pubkey, resolver: f.other.resolver },
          (c) => mutate(qualified(c))
        )
      ).toMatchObject({ newListingReady: false, sendReady: false });
    }
  } finally {
    f.close();
  }
});
it('stale private session, forged own resolver and trusted observer invalidation cannot retain readiness', async () => {
  const f = await fixture();
  try {
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        {} as InboxResolver,
        undefined,
        qualified
      ).newListingReady
    ).toBe(false);
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        f.own.resolver,
        undefined,
        (c) => {
          f.own.stale();
          return qualified(c);
        }
      ).newListingReady
    ).toBe(false);
    if (f.session) closePrivateSession(f.session);
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        f.own.resolver,
        undefined,
        qualified
      ).newListingReady
    ).toBe(false);
  } finally {
    f.close();
  }
});

it('explicit private closure blocks readiness while identity remains messaging capable', async () => {
  const f = await fixture();
  try {
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        f.own.resolver,
        undefined,
        qualified
      ).newListingReady
    ).toBe(true);
    if (f.session) closePrivateSession(f.session);
    expect(identitySessionSnapshot(f.identity).state).toBe('messaging_capable');
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        f.own.resolver,
        undefined,
        qualified
      ).newListingReady
    ).toBe(false);
  } finally {
    f.close();
  }
});
it('mutating detached qualification context cannot retarget expected receiving sources', async () => {
  const f = await fixture();
  try {
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        policy(),
        f.own.resolver,
        undefined,
        (c) => {
          (c.readTargets as string[]).push('wss://hint.example.org');
          return qualified(c);
        }
      ).newListingReady
    ).toBe(false);
  } finally {
    f.close();
  }
});
it('disabled messaging never asks a trusted qualification port to grant readiness', async () => {
  const f = await fixture();
  let called = 0;
  try {
    const disabled = validateRelayPolicy(
      JSON.stringify({
        schemaVersion: 1,
        public: [{ origin: discovery, read: true, write: false, nip50: false }],
        inbox: [ownOrigin, peerOrigin].map((origin) => ({
          origin,
          read: true,
          write: true
        })),
        postingEnabled: true,
        messagingEnabled: false,
        operatorDenylist: []
      })
    )!;
    expect(
      messagingReadinessSnapshot(
        f.identity,
        f.session,
        disabled,
        f.own.resolver,
        undefined,
        (c) => {
          called++;
          return qualified(c);
        }
      )
    ).toMatchObject({
      ownConfiguration: 'compatible',
      newListingReady: false,
      sendReady: false
    });
    expect(called).toBe(0);
  } finally {
    f.close();
  }
});
