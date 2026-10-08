import {
  buildPrivateGiftwrap,
  privateGiftwrapSnapshot
} from '../../../src/lib/nostr/giftwrap-builder.ts';
export { buildPrivateGiftwrap, privateGiftwrapSnapshot };
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
  verifiedSymbol,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { verifyEnvelope } from '../../../src/lib/nostr/verified-envelope.ts';
import { captureEnquiryContext } from '../../../src/lib/messaging/enquiry-context.ts';
import { captureEnquiryRumor } from '../../../src/lib/messaging/rumor-plan.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import { createPrivateSendReservationRepository } from '../../../src/lib/persistence/private-send-reservations.ts';
import {
  reserveSendIdentity,
  reservedSendRumorWire
} from '../../../src/lib/messaging/send-identity.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  identitySessionSnapshot,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  capturePrivateSealOperation,
  buildPrivateSeal,
  stopPrivateSeal,
  expirePrivateSealWait,
  privateSealSnapshot
} from '../../../src/lib/nostr/seal-builder.ts';
import {
  browserExtensionScheduler,
  extensionSchedulerSnapshot
} from '../../../src/lib/nostr/extension-scheduler.ts';
export {
  capturePrivateSealOperation,
  buildPrivateSeal,
  stopPrivateSeal,
  expirePrivateSealWait,
  privateSealSnapshot
};
export type Mode =
  | 'normal'
  | 'wrong_cipher'
  | 'wrong_author'
  | 'wrong_tags'
  | 'wrong_time'
  | 'cached_bad_id'
  | 'missing'
  | 'declined'
  | 'changed_key'
  | 'hold_encrypt'
  | 'hold_sign'
  | 'wrong_plaintext'
  | 'declined_decrypt'
  | 'hold_decrypt'
  | 'getter_reentry'
  | 'stop_capability_getter'
  | 'stop_capability_after_key';
// Ephemeral source-only provider: actual stock SDK encryption/signature and real
// Chromium IDB/WebLocks. No installed extension/relay/client qualification.
export async function makeFixture(
  inputText: unknown = 'Private seal sentinel'
) {
  const secret = generateSecretKey(),
    peerSecret = generateSecretKey(),
    owner = getPublicKey(secret),
    peer = getPublicKey(peerSecret);
  let mode: Mode = 'normal',
    keys = 0,
    encrypts = 0,
    signs = 0,
    decrypts = 0,
    capabilityReads = 0,
    reentrantReads = 0,
    postStopEncrypts = 0,
    getterStopped = false,
    watched: ReturnType<typeof capturePrivateSealOperation>,
    release: (() => void) | undefined;
  function encryptTo(target: string, text: string) {
    const conversation = nip44.v2.utils.getConversationKey(secret, target);
    try {
      return nip44.v2.encrypt(text, conversation);
    } finally {
      conversation.fill(0);
    }
  }
  function sign(template: UnsignedEvent) {
    const input =
      mode === 'wrong_cipher'
        ? { ...template, content: encryptTo(peer, 'different immutable rumor') }
        : mode === 'wrong_tags'
          ? { ...template, tags: [['client', 'leak']] }
          : mode === 'wrong_time'
            ? { ...template, created_at: template.created_at + 1 }
            : template;
    const signed = finalizeEvent(
      input,
      mode === 'wrong_author' ? peerSecret : secret
    );
    return mode === 'cached_bad_id'
      ? { ...signed, id: '0'.repeat(64), [verifiedSymbol]: true }
      : {
          ...signed,
          plaintext: 'never retained sentinel',
          toJSON: () => {
            throw Error('unexpected provider conversion');
          }
        };
  }
  const provider = {
    getPublicKey: () => {
      keys++;
      return Promise.resolve(mode === 'changed_key' ? peer : owner);
    },
    signEvent: (template: UnsignedEvent) => {
      signs++;
      return mode === 'hold_sign'
        ? new Promise<ReturnType<typeof sign>>((resolve) => {
            release = () => resolve(sign(template));
          })
        : Promise.resolve(sign(template));
    },
    get nip44() {
      capabilityReads++;
      if (
        watched &&
        (mode === 'stop_capability_getter' ||
          (mode === 'stop_capability_after_key' && capabilityReads === 2))
      ) {
        stopPrivateSeal(watched);
        return undefined;
      }
      return mode === 'missing'
        ? undefined
        : {
            get encrypt() {
              const scheduler = browserExtensionScheduler(),
                slot = scheduler && extensionSchedulerSnapshot(scheduler);
              if (
                mode === 'getter_reentry' &&
                watched &&
                slot &&
                slot.state === 'active' &&
                slot.pending === 'encrypt'
              ) {
                reentrantReads++;
                getterStopped = true;
                stopPrivateSeal(watched);
              }
              return (target: string, text: string) => {
                encrypts++;
                if (getterStopped) postStopEncrypts++;
                if (mode === 'declined')
                  return Promise.reject(Error('private failure must not leak'));
                if (mode === 'hold_encrypt')
                  return new Promise<string>((resolve) => {
                    release = () => resolve(encryptTo(target, text));
                  });
                return Promise.resolve(
                  encryptTo(
                    target,
                    mode === 'wrong_plaintext' ? 'substitute rumor' : text
                  )
                );
              };
            },
            decrypt: (target: string, cipher: string) => {
              decrypts++;
              if (mode === 'declined_decrypt')
                return Promise.reject(Error('decrypt refused'));
              const decryptNow = () => {
                const conversation = nip44.v2.utils.getConversationKey(
                  secret,
                  target
                );
                try {
                  return nip44.v2.decrypt(cipher, conversation);
                } finally {
                  conversation.fill(0);
                }
              };
              if (mode === 'hold_decrypt')
                return new Promise<string>((resolve) => {
                  release = () => resolve(decryptNow());
                });
              return Promise.resolve(decryptNow());
            }
          };
    }
  };
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: provider
  });
  const identity = createIdentitySession();
  await connectIdentity(identity);
  await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const food = finalizeEvent(
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
    peerSecret
  );
  const proof = verifyEnvelope(JSON.stringify(food));
  if (!proof.ok) throw Error('invalid public fixture');
  const context = captureEnquiryContext(proof.value);
  if (!context) throw Error('invalid context');
  const reservationTime = Date.now();
  const plan = captureEnquiryRumor(
    identity,
    context,
    inputText,
    Math.floor(reservationTime / 1000)
  );
  if (!plan) throw Error('missing plan');
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error('missing database');
  const database = opened.owner,
    repository = createPrivateSendReservationRepository(database, owner);
  if (!repository) throw Error('missing repository');
  // Controlled fixture second across actual asynchronous IDB/WebLock setup.
  // Production admission still requires its real current observed second.
  const liveNow = Date.now;
  let reservation: Awaited<ReturnType<typeof reserveSendIdentity>>;
  Date.now = () => reservationTime;
  try {
    reservation = await reserveSendIdentity(
      repository,
      identity,
      '12345678-1234-4234-8234-123456789abc',
      plan,
      'reviewed_private_intent'
    );
  } finally {
    Date.now = liveNow;
  }
  if (reservation.status !== 'reserved')
    throw Error('missing reservation: ' + reservation.status);
  const reserved = reservation.identity;
  const original = reservedSendRumorWire(reserved);
  return {
    owner,
    peer,
    identity,
    reserved,
    textPlan: (text: unknown) =>
      captureEnquiryRumor(
        identity,
        context,
        text,
        Math.floor(Date.now() / 1000)
      ),
    async reserveAgain() {
      const result = await reserveSendIdentity(
        repository,
        identity,
        '12345678-1234-4234-8234-123456789abc',
        plan,
        'reviewed_private_intent'
      );
      if (result.status !== 'existing' && result.status !== 'reserved')
        throw Error('missing second reservation');
      return result.identity;
    },
    operation: (
      role: unknown = 'peer',
      review: unknown = 'reviewed_private_seal'
    ) => {
      watched = capturePrivateSealOperation(identity, reserved, role, review);
      return watched;
    },
    mode: (value: Mode) => {
      mode = value;
      capabilityReads = 0;
    },
    counts: () => ({
      keys,
      encrypts,
      decrypts,
      signs,
      reentrantReads,
      postStopEncrypts
    }),
    identityState: () => identitySessionSnapshot(identity).state,
    settle: () => {
      release?.();
    },
    slot: () => {
      const scheduler = browserExtensionScheduler();
      return scheduler ? extensionSchedulerSnapshot(scheduler) : undefined;
    },
    disconnect: () => disconnectIdentity(identity),
    async stored() {
      const tx = browserDatabaseTransaction(
          database,
          ['private_sends'],
          'readonly'
        ),
        request = tx.objectStore('private_sends').getAll();
      return await new Promise<string>((resolve, reject) => {
        tx.addEventListener('complete', () =>
          resolve(JSON.stringify(request.result))
        );
        tx.addEventListener('abort', () => reject(Error('inspection aborted')));
      });
    },
    decryptWrap(wire: string, role: 'peer' | 'self') {
      const wrap = JSON.parse(wire) as { pubkey: string; content: string };
      const conversation = nip44.v2.utils.getConversationKey(
        role === 'peer' ? peerSecret : secret,
        wrap.pubkey
      );
      try {
        return nip44.v2.decrypt(wrap.content, conversation);
      } finally {
        conversation.fill(0);
      }
    },
    decrypt(wire: string, role: 'peer' | 'self') {
      const seal = JSON.parse(wire) as { content: string };
      const conversation = nip44.v2.utils.getConversationKey(
        role === 'peer' ? peerSecret : secret,
        owner
      );
      try {
        return {
          equal: nip44.v2.decrypt(seal.content, conversation) === original,
          seal
        };
      } finally {
        conversation.fill(0);
      }
    },
    close() {
      disconnectIdentity(identity);
      closeBrowserDatabase(database);
      secret.fill(0);
      peerSecret.fill(0);
      delete window.nostr;
    }
  };
}
