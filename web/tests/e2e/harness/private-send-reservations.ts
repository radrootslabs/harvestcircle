import { finalizeEvent, type NostrEvent } from 'applesauce-core/helpers';
import { verifyEnvelope } from '../../../src/lib/nostr/verified-envelope.ts';
import { captureEnquiryContext } from '../../../src/lib/messaging/enquiry-context.ts';
import {
  captureEnquiryRumor,
  type RumorPlan
} from '../../../src/lib/messaging/rumor-plan.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import { createPrivateSendReservationRepository } from '../../../src/lib/persistence/private-send-reservations.ts';
import {
  reserveSendIdentity,
  reservedSendSnapshot,
  reservedSendRumorWire
} from '../../../src/lib/messaging/send-identity.ts';
export { reserveSendIdentity, reservedSendSnapshot, reservedSendRumorWire };
export const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
export const command = '12345678-1234-4234-8234-123456789abc';
export type Shared = { event: NostrEvent; milliseconds: number };
// Controlled source-only clock/provider fixtures; real browser IDB/WebLocks,
// never installed extensions, cryptographic messaging or real clock Q evidence.
export async function makeFixture(shared?: Shared) {
  const previousNow = Date.now;
  let milliseconds = shared?.milliseconds ?? previousNow(),
    signs = 0,
    encrypts = 0,
    decrypts = 0;
  Date.now = () => milliseconds;
  function publicFood() {
    const key = crypto.getRandomValues(new Uint8Array(32));
    try {
      return finalizeEvent(
        {
          kind: 30402,
          created_at: 1700000060,
          content: 'Public carrots',
          tags: [
            ['d', 'carrots'],
            ['title', 'Carrots'],
            ['summary', 'Fresh carrots'],
            ['published_at', '1700000000'],
            ['location', 'Victoria'],
            ['price', '3.5', 'CAD'],
            ['radroots:price_unit', 'lb'],
            ['status', 'active']
          ]
        },
        key
      );
    } finally {
      key.fill(0);
    }
  }
  const event = shared?.event ?? publicFood(),
    proof = verifyEnvelope(JSON.stringify(event));
  if (!proof.ok) throw Error('invalid public source fixture');
  const context = captureEnquiryContext(proof.value);
  if (!context) throw Error('invalid Food context');
  const capturedContext = context;
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () => Promise.resolve(owner),
      signEvent: () => {
        signs++;
        return Promise.reject(Error('unexpected sign'));
      },
      nip44: {
        encrypt: (_peer: string, text: string) => {
          encrypts++;
          return Promise.resolve('fixture:' + text);
        },
        decrypt: (_peer: string, text: string) => {
          decrypts++;
          return Promise.resolve(text.slice(8));
        }
      }
    }
  });
  const identity = createIdentitySession();
  await connectIdentity(identity);
  await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready')
    throw Error('browser fixture database unavailable');
  const database = opened.owner,
    repository = createPrivateSendReservationRepository(database, owner);
  if (!repository) throw Error('invalid source repository');
  function plan(
    text = 'Private reservation sentinel',
    seconds = Math.floor(milliseconds / 1000)
  ) {
    const value = captureEnquiryRumor(identity, capturedContext, text, seconds);
    if (!value) throw Error('expected local source plan');
    return value;
  }
  async function rows() {
    const transaction = browserDatabaseTransaction(
        database,
        ['private_sends'],
        'readonly'
      ),
      request = transaction.objectStore('private_sends').getAll();
    return await new Promise<unknown[]>((resolve, reject) => {
      transaction.addEventListener('complete', () =>
        resolve(request.result as unknown[])
      );
      transaction.addEventListener('abort', () =>
        reject(Error('inspection aborted'))
      );
    });
  }
  async function inject(value: unknown) {
    const transaction = browserDatabaseTransaction(
      database,
      ['private_sends'],
      'readwrite'
    );
    transaction.objectStore('private_sends').put(value);
    await new Promise<void>((resolve, reject) => {
      transaction.addEventListener('complete', () => resolve());
      transaction.addEventListener('abort', () =>
        reject(Error('fixture injection aborted'))
      );
    });
  }
  return {
    identity,
    repository,
    database,
    event,
    plan,
    rows,
    inject,
    shared: (): Shared => ({ event, milliseconds }),
    setTime: (value: number) => {
      milliseconds = value;
    },
    counts: () => ({ signs, encrypts, decrypts }),
    reserve: (id: string, value: RumorPlan) =>
      reserveSendIdentity(
        repository,
        identity,
        id,
        value,
        'reviewed_private_intent'
      ),
    disconnect: () => disconnectIdentity(identity),
    close() {
      disconnectIdentity(identity);
      closeBrowserDatabase(database);
      Date.now = previousNow;
    }
  };
}
