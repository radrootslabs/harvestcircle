import { makeFixture } from './approved-signing.ts';
import { fixturePolicy, inbox } from './inbox-setup.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  createPrivateSession,
  closePrivateSession
} from '../../../src/lib/runtime/private-session.ts';
import {
  getPrivatePool,
  beginPrivateAuthConnection,
  privateAuthChallenge,
  reservePrivateAuthResponse,
  sendPrivateAuthResponse,
  subscribePrivatePage
} from '../../../src/lib/nostr/private-pool.ts';
import { RelayPool } from 'applesauce-relay';
export {
  beginInboxAuthentication,
  respondInboxAuthentication,
  inboxAuthSnapshot,
  closeInboxAuthentication
} from '../../../src/lib/nostr/inbox-auth.ts';
export {
  browserExtensionScheduler,
  extensionSchedulerSnapshot
} from '../../../src/lib/nostr/extension-scheduler.ts';
export {
  getPublicPool,
  subscribePublicPool,
  closePublicPool
} from '../../../src/lib/nostr/public-pool.ts';
export { inbox, fixturePolicy };
export async function makeAuthFixture() {
  const fixture = makeFixture(10050),
    identity = createIdentitySession();
  let mode: 'exact' | 'author' | 'relay' | 'challenge' | 'cached' | 'denied' =
      'exact',
    signs = 0,
    changed = false;
  let before = () => Promise.resolve();
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () =>
        Promise.resolve(changed ? 'f'.repeat(64) : fixture.owner),
      signEvent: async (input: Parameters<typeof fixture.sign>[0]) => {
        signs++;
        await before();
        if (mode === 'relay') input.tags[0][1] = 'wss://wrong.example.org/';
        if (mode === 'challenge') input.tags[1][1] = 'wrong';
        return fixture.sign(
          input,
          mode === 'relay' || mode === 'challenge' ? 'exact' : mode
        );
      },
      nip44: {
        encrypt: (_peer: string, value: string) =>
          Promise.resolve('fixture:' + value),
        decrypt: (_peer: string, value: string) =>
          Promise.resolve(value.slice(8))
      }
    }
  });
  await connectIdentity(identity);
  await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const session = await createPrivateSession(
    identity,
    'reviewed_private_session'
  );
  if (!session) throw new Error('missing genuine private session');
  const policy = fixturePolicy(),
    pool = getPrivatePool(session, policy, [inbox]);
  if (!pool) throw new Error('missing private pool');
  return {
    pool,
    policy,
    owner: fixture.owner,
    signs: () => signs,
    mode: (next: typeof mode) => {
      mode = next;
    },
    beforeSign: (next: typeof before) => {
      before = next;
    },
    changeOwner: () => {
      changed = true;
    },
    logout: () => disconnectIdentity(identity),
    reopen: () => getPrivatePool(session, policy, [inbox]),
    startPage: () => subscribePrivatePage(pool, 1, () => {}),
    async rawPortBypass() {
      const connection = (await Reflect.apply(
        beginPrivateAuthConnection,
        undefined,
        [pool, inbox]
      )) as Awaited<ReturnType<typeof beginPrivateAuthConnection>>;
      if (!connection) return 'blocked';
      for (let i = 0; i < 100; i++) {
        const proof = privateAuthChallenge(connection);
        if (proof) {
          if (!reservePrivateAuthResponse(connection, proof)) return 'blocked';
          const event = fixture.sign({
            kind: 22242,
            created_at: Math.floor(Date.now() / 1000),
            tags: [
              ['relay', proof.relay],
              ['challenge', proof.challenge]
            ],
            content: ''
          });
          return (await Reflect.apply(sendPrivateAuthResponse, undefined, [
            connection,
            proof,
            JSON.stringify(event)
          ])) as string;
        }
        await new Promise<void>((resolve) => setTimeout(resolve, 10));
      }
      throw new Error('missing real challenge for bypass test');
    },
    close: () => {
      closePrivateSession(session);
      disconnectIdentity(identity);
      fixture.close();
    }
  };
}
export function installCloseFailure() {
  const original = Reflect.get<RelayPool, 'close'>(
    RelayPool.prototype,
    'close'
  );
  let failing = true;
  RelayPool.prototype.close = function (
    ...args: Parameters<RelayPool['close']>
  ) {
    if (failing) throw new Error('controlled physical cleanup fault');
    return original.apply(this, args);
  };
  return {
    allow: () => {
      failing = false;
    },
    restore: () => {
      RelayPool.prototype.close = original;
    }
  };
}
