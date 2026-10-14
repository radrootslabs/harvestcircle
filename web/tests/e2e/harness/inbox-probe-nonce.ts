import { makeReadbackFixture } from './inbox-probe-readback.ts';
import {
  captureInboxProbeNonceReader,
  readInboxProbeNonce,
  inboxProbeNonceProofBinding,
  stopInboxProbeNonceReader,
  type InboxProbeNonceReader,
  type InboxProbeNonceProof
} from '../../../src/lib/nostr/self-recovery-reader.ts';
import type { UnsignedEvent } from 'applesauce-core/helpers';
export * from './inbox-probe-readback.ts';
export * from '../../../src/lib/nostr/self-recovery-reader.ts';
export * from '../../../src/lib/nostr/extension-scheduler.ts';
export * from '../../../src/lib/runtime/identity-session.ts';
export async function makeNonceFixture() {
  const f = await makeReadbackFixture();
  let reader: InboxProbeNonceReader | undefined,
    proof: InboxProbeNonceProof | undefined;
  const restores = new Map<symbol, () => void>();
  function capture(review: unknown = 'reviewed_inbox_probe_nonce_decryption') {
    const observed = f.lastRead();
    return observed
      ? captureInboxProbeNonceReader(
          f.repository,
          f.identity,
          f.session,
          f.policy,
          observed,
          review
        )
      : undefined;
  }
  type Provider = {
    getPublicKey(): Promise<string>;
    signEvent(input: UnsignedEvent): Promise<Record<string, unknown>>;
    nip44: { decrypt(target: string, content: string): Promise<string> };
  };
  const provider = Reflect.get(window, 'nostr') as Provider;
  function retain(restore: () => void) {
    const id = Symbol();
    restores.set(id, restore);
    return () => {
      restore();
      restores.delete(id);
    };
  }
  return {
    ...f,
    nonceCapture: capture,
    nonceReader: () => reader,
    nonceProof: () => proof,
    proofBinding: () =>
      proof &&
      inboxProbeNonceProofBinding(proof, f.repository, f.session, f.policy),
    async nonce(selected = (reader ??= capture())) {
      if (!selected) return { status: 'invalid' as const };
      reader ??= selected;
      const result = await readInboxProbeNonce(selected);
      if ('proof' in result) proof = result.proof;
      return {
        status: result.status,
        proof: 'proof' in result ? result.proof : undefined
      };
    },
    keyCallback(hit: number, callback: () => void | Promise<void>) {
      const original = Object.getOwnPropertyDescriptor(
        provider,
        'getPublicKey'
      )!.value as Provider['getPublicKey'];
      let n = 0;
      provider.getPublicKey = async function () {
        n++;
        if (n === hit) await callback();
        return original.call(this);
      };
      return retain(() => {
        provider.getPublicKey = original;
      });
    },
    cipherGetter(callback: () => void) {
      const descriptor = Object.getOwnPropertyDescriptor(provider, 'nip44')!,
        cipher = provider.nip44;
      Object.defineProperty(provider, 'nip44', {
        configurable: true,
        get() {
          callback();
          return cipher;
        }
      });
      return retain(() => {
        Object.defineProperty(provider, 'nip44', descriptor);
      });
    },
    decryptCallback(
      hit: number,
      transform: (wire: string) => string | Promise<string>
    ) {
      const cipher = provider.nip44,
        descriptor = Object.getOwnPropertyDescriptor(cipher, 'decrypt')!,
        original = cipher.decrypt.bind(cipher);
      let n = 0,
        receivers = 0;
      Object.defineProperty(cipher, 'decrypt', {
        configurable: true,
        value: async function (
          this: Provider['nip44'],
          target: string,
          content: string
        ) {
          if (this !== cipher) throw Error('wrong actual cipher receiver');
          receivers++;
          const wire = await original(target, content);
          n++;
          return n === hit ? transform(wire) : wire;
        }
      });
      return {
        receivers: () => receivers,
        restore: retain(() => {
          Object.defineProperty(cipher, 'decrypt', descriptor);
        })
      };
    },
    async changedNativeFact() {
      const rows = await f.stored(),
        row = rows.find((x) => x.id === f.snapshot()!.record.id)!;
      const record = JSON.parse(row.wire) as {
        revision: number;
        receipts: unknown[];
      };
      record.revision++;
      record.receipts.push(record.receipts[0]);
      await f.inject([{ ...row, wire: JSON.stringify(record) }]);
    },
    async signSeal(wire: string, change: Record<string, unknown>) {
      const seal = JSON.parse(wire) as UnsignedEvent;
      const event = await provider.signEvent({
        pubkey: seal.pubkey,
        kind: seal.kind,
        created_at: seal.created_at,
        tags: seal.tags,
        content: seal.content,
        ...change
      });
      return JSON.stringify(event);
    },
    close() {
      if (reader) stopInboxProbeNonceReader(reader);
      for (const [id, off] of restores) {
        off();
        restores.delete(id);
      }
      f.close();
    }
  };
}
