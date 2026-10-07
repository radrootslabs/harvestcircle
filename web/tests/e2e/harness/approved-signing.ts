import {
  finalizeEvent,
  getEventHash,
  verifiedSymbol,
  type EventTemplate
} from 'applesauce-core/helpers';
import {
  decodePublicRecord,
  publicRecordSnapshot
} from '../../../src/lib/persistence/records.ts';
import { capturedArtifactSnapshot } from '../../../src/lib/persistence/artifact-records.ts';
import {
  approveCapturedPublicSigning,
  disposableApprovedTemplate,
  bindApprovedResponse
} from '../../../src/lib/nostr/approved-signing.ts';
export {
  createExtensionAdapter,
  connectExtensionAdapter,
  disconnectExtensionAdapter,
  signApprovedExtensionAdapter,
  extensionSnapshot
} from '../../../src/lib/nostr/extension.ts';
export {
  approveCapturedPublicSigning,
  disposableApprovedTemplate,
  bindApprovedResponse,
  capturedArtifactSnapshot,
  publicRecordSnapshot
};
export type Mode =
  | 'exact'
  | 'author'
  | 'kind'
  | 'time'
  | 'tags'
  | 'content'
  | 'cached'
  | 'mutate'
  | 'denied';
// Test-only ephemeral cryptographic material. No literal key, persistence, log,
// production dependency or named-extension qualification is introduced.
export function makeFixture(kind: 30402 | 5 | 10050 = 30402) {
  const key = crypto.getRandomValues(new Uint8Array(32));
  const source = finalizeEvent(
    {
      kind: kind === 10050 ? 10050 : 30402,
      created_at: 100,
      tags:
        kind === 10050
          ? []
          : [
              ['d', 'fixture'],
              ['published_at', '100']
            ],
      content: 'public fixture source'
    },
    key
  );
  const template = {
    pubkey: source.pubkey,
    kind,
    created_at: 101,
    tags:
      kind === 5
        ? [
            ['e', source.id],
            ['a', `30402:${source.pubkey}:fixture`]
          ]
        : kind === 10050
          ? []
          : [
              ['d', 'fixture'],
              ['published_at', '100']
            ],
    content: 'public fixture operation'
  };
  const id = crypto.randomUUID();
  const row = {
    schema: 1,
    family: kind === 10050 ? 'preference_operation' : 'public_operation',
    owner: source.pubkey,
    id,
    revision: 0,
    ...(kind === 10050 ? { consent: 'explicit_review' } : {}),
    source: {
      type: kind === 10050 ? 'inbox_head' : 'public_head',
      wire: JSON.stringify(source)
    },
    capture: {
      kind,
      wire: JSON.stringify(template),
      hash: getEventHash(template),
      targets: ['wss://one.example.org'],
      policyFingerprint: 'a'.repeat(64)
    },
    artifact: null,
    receipts: []
  };
  const record = decodePublicRecord(JSON.stringify(row), row.owner, id);
  if (!record.ok) {
    key.fill(0);
    throw new Error('invalid test fixture');
  }
  return {
    record: record.value,
    owner: row.owner,
    id,
    template,
    sign(input: EventTemplate, mode: Mode = 'exact') {
      if (mode === 'denied') throw new Error('test-only refusal');
      const copy = { ...input, tags: input.tags.map((tag) => [...tag]) };
      if (mode === 'kind') copy.kind = 1;
      if (mode === 'time') copy.created_at++;
      if (mode === 'tags' || mode === 'mutate')
        copy.tags.push(['changed', 'fixture']);
      if (mode === 'content') copy.content += ' changed';
      if (mode === 'mutate') {
        input.tags.push(['provider', 'mutation']);
        input.content = 'changed disposable copy';
      }
      if (mode === 'author') {
        const other = crypto.getRandomValues(new Uint8Array(32));
        try {
          return finalizeEvent(copy, other);
        } finally {
          other.fill(0);
        }
      }
      const event = finalizeEvent(copy, key);
      if (mode === 'cached') event.sig = '0'.repeat(128);
      // A hostile provider can return the SDK's mutable cached verification flag.
      Object.defineProperty(event, verifiedSymbol, {
        value: true,
        configurable: true
      });
      return event;
    },
    close() {
      key.fill(0);
    }
  };
}
