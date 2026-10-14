import { makeAuthFixture } from './inbox-auth.ts';
import { validateRelayPolicy } from '../../../src/lib/config/relays.ts';
import {
  createPublicRuntimeContext,
  mountPublicRuntime,
  closePublicRuntime,
  createPublicView,
  beginPublicViewRun,
  disposePublicView
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  resolveInboxPreference,
  settleInboxResolver,
  inboxResolverCurrentAfterSample,
  type InboxResolver
} from '../../../src/lib/messaging/resolve-inbox.ts';
import {
  createInboxAccessController,
  closeInboxAccessController
} from '../../../src/lib/messaging/inbox-access-controller.ts';
import {
  getPrivatePool,
  subscribePrivatePage,
  subscribePrivateLive,
  closePrivatePool,
  type PrivatePool
} from '../../../src/lib/nostr/private-pool.ts';
export * from '../../../src/lib/messaging/inbox-access-controller.ts';
export { installCloseFailure } from './inbox-auth.ts';
import {
  createPrivateSession,
  closePrivateSession,
  type PrivateSession
} from '../../../src/lib/runtime/private-session.ts';
export const origins = [
  'wss://inbox.example.org',
  'wss://peer.example.org',
  'wss://archive.example.org'
];
// Genuine actors/SDK discovery/AUTH with controlled loopback/provided identity;
// no installed extension, access probe, operator or full qualification claim.
export async function makeControllerFixture(actualCrypto = false) {
  const capturedPolicy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://discovery.example.org',
          read: true,
          write: true,
          nip50: false
        }
      ],
      inbox: origins.map((origin) => ({ origin, read: true, write: true })),
      postingEnabled: true,
      messagingEnabled: true,
      operatorDenylist: []
    })
  );
  if (!capturedPolicy) throw Error('fixture policy');
  const policy = capturedPolicy;
  const fixture = await makeAuthFixture(policy, actualCrypto);
  const context = createPublicRuntimeContext(),
    runtime = mountPublicRuntime(context, policy);
  if (!runtime) throw Error('actual public runtime');
  const view = createPublicView(runtime);
  let controller: ReturnType<typeof createInboxAccessController>,
    own: InboxResolver | undefined,
    successor: PrivateSession | undefined;
  const pools = new Map<PrivatePool, true>();
  function scope(
    selected: string[],
    role: 'receive' | 'delivery',
    bindHead = false
  ) {
    const pool = getPrivatePool(
      fixture.session,
      policy,
      selected,
      Object.freeze({}),
      role,
      bindHead
        ? () => !!own && inboxResolverCurrentAfterSample(own)
        : () => true
    );
    if (!pool) throw Error('actual scoped private pool');
    pools.set(pool, true);
    return pool;
  }
  return {
    ...fixture,
    async capture() {
      const run = beginPublicViewRun(view);
      own = resolveInboxPreference(view, run, fixture.owner);
      await settleInboxResolver(own);
      controller = createInboxAccessController(fixture.session, policy, own);
      if (!controller) throw Error('actual access controller');
      return controller;
    },
    churnHead() {
      beginPublicViewRun(view);
    },
    scope,
    page(pool: PrivatePool, ended: (reason: string) => void = () => {}) {
      return subscribePrivatePage(pool, 1, (message) => {
        if (message.type === 'end') ended(message.reason);
      });
    },
    live(
      pool: PrivatePool,
      onOpen: () => void,
      candidate: () => void = () => {}
    ) {
      return subscribePrivateLive(pool, (message) => {
        if (message.type === 'open') onOpen();
        if (message.type === 'candidate') candidate();
      });
    },
    closeSession() {
      return closePrivateSession(fixture.session);
    },
    async successorScope() {
      successor ??= await createPrivateSession(
        fixture.identity,
        'reviewed_private_session'
      );
      if (!successor) throw Error('genuine successor session unavailable');
      const pool = getPrivatePool(
        successor,
        policy,
        origins,
        Object.freeze({}),
        'receive'
      );
      if (!pool) throw Error('genuine successor pool unavailable');
      pools.set(pool, true);
      return pool;
    },
    stopScope(pool: PrivatePool) {
      closePrivatePool(pool);
    },
    close() {
      if (controller) closeInboxAccessController(controller);
      for (const pool of pools.keys()) closePrivatePool(pool);
      if (successor) closePrivateSession(successor);
      disposePublicView(view);
      closePublicRuntime(context);
      fixture.close();
    }
  };
}
