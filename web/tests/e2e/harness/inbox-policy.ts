import { Relay } from 'applesauce-relay';
import { PrivateKeySigner } from 'applesauce-signers';
import { getEventHash, type NostrEvent } from 'applesauce-core/helpers';
import { NEVER, firstValueFrom, filter, take, toArray } from 'rxjs';

// Test-owned actual SDK cryptography/transport, never a product signer or relay.
const url = 'wss://inbox.example.org/';
export async function makePolicyFixture() {
  const sender = new PrivateKeySigner(),
    recipient = new PrivateKeySigner();
  const owner = await sender.getPublicKey(),
    peer = await recipient.getPublicKey();
  const now = Math.floor(Date.now() / 1000);
  const rumorFields = {
    kind: 14,
    pubkey: owner,
    created_at: now,
    tags: [['p', peer]],
    content: 'loopback policy fixture'
  };
  const rumor = { ...rumorFields, id: getEventHash(rumorFields) };
  async function wrap(destination: string) {
    const seal = await sender.signEvent({
      kind: 13,
      created_at: now - 300,
      tags: [],
      content: await sender.nip44.encrypt(destination, JSON.stringify(rumor))
    });
    const disposable = new PrivateKeySigner();
    try {
      return await disposable.signEvent({
        kind: 1059,
        created_at: now - 48 * 3600 - 300,
        tags: [['p', destination]],
        content: await disposable.nip44.encrypt(
          destination,
          JSON.stringify(seal)
        )
      });
    } finally {
      disposable.key.fill(0);
    }
  }
  const wraps = { recipient: await wrap(peer), archive: await wrap(owner) };
  async function client(signer: PrivateKeySigner, authenticate: boolean) {
    const relay = new Relay(url, {
      keepAlive: 0,
      enablePing: false,
      requestReconnect: 0,
      publishRetry: { count: 0 }
    });
    const hold = relay
      .req(NEVER, { waitForAuth: false, reconnect: false, resubscribe: false })
      .subscribe();
    const challenge = await firstValueFrom(
      relay.challenge$.pipe(
        filter((v): v is string => typeof v === 'string'),
        take(1)
      )
    );
    if (authenticate) {
      const event = await signer.signEvent({
        kind: 22242,
        created_at: now,
        tags: [
          ['relay', url],
          ['challenge', challenge]
        ],
        content: ''
      });
      const result = await relay.auth(event);
      if (!result.ok) throw new Error('fixture AUTH refused');
    }
    return {
      relay,
      close() {
        hold.unsubscribe();
        relay.close();
      }
    };
  }
  const clients: Awaited<ReturnType<typeof client>>[] = [];
  async function connect(which: 'sender' | 'recipient', authenticate = true) {
    const c = await client(
      which === 'sender' ? sender : recipient,
      authenticate
    );
    clients.push(c);
    return c;
  }
  async function read(c: Awaited<ReturnType<typeof client>>, target: string) {
    try {
      const events = await firstValueFrom(
        c.relay
          .request(
            { kinds: [1059], '#p': [target], limit: 200 },
            {
              waitForAuth: false,
              reconnect: false,
              resubscribe: false,
              timeout: 2000
            }
          )
          .pipe(toArray())
      );
      return { status: 'complete', ids: events.map((e) => e.id) };
    } catch {
      return { status: 'refused', ids: [] };
    }
  }
  async function write(
    c: Awaited<ReturnType<typeof client>>,
    event: NostrEvent
  ) {
    const result = await firstValueFrom(c.relay.event(event, 'EVENT'));
    return { accepted: result.ok };
  }
  return {
    owner,
    peer,
    wraps,
    connect,
    read,
    write,
    close() {
      for (const c of clients) c.close();
      sender.key.fill(0);
      recipient.key.fill(0);
    }
  };
}
