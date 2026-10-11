import { naddrEncode, neventEncode } from 'applesauce-core/helpers/pointers';
import {
  messageToWireParts,
  messageFromWireParts,
  type MessageV1
} from '../../src/lib/contracts/message-v1/index.ts';

// Separate browser-owned recipes. These pure projections grant no room,
// participant, extension, encryption, persistence or publication authority.
const peer = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
const parent = '1'.repeat(64);
const addressA = naddrEncode({
  kind: 30402,
  pubkey: peer,
  identifier: 'carrots-a'
});
const addressB = naddrEncode({
  kind: 30402,
  pubkey: peer,
  identifier: 'carrots-b'
});
const versionA = neventEncode({
  id: '3'.repeat(64),
  author: peer,
  kind: 30402
});
const base: MessageV1 = {
  recipients: [{ public_key: peer, relay_url: null }],
  content: 'Could I buy carrots?\nPlease reply here.',
  reply_to: null,
  subject: null
};
export const qualifiedMessages: readonly MessageV1[] = [
  base,
  {
    ...base,
    recipients: [{ public_key: peer, relay_url: 'wss://hint.example.org' }]
  },
  { ...base, reply_to: { id: parent, relays: null } },
  { ...base, reply_to: { id: parent, relays: 'wss://hint.example.org' } },
  { ...base, subject: 'Carrots' },
  { ...base, reply_to: { id: parent, relays: null }, subject: 'Carrots' },
  { ...base, content: 'Fresh café' },
  { ...base, content: 'Fresh cafe\u0301' },
  { ...base, content: 'Line one\r\nLine two\rLine three\n' },
  { ...base, content: '🥕'.repeat(1024) },
  { ...base, content: '  Please keep these spaces.  ' },
  { ...base, content: '\ufeff' },
  { ...base, content: '\u001c' },
  { ...base, subject: '🥕'.repeat(160) },
  {
    ...base,
    content: `Enquiry context: nostr:${addressA}\nViewed version: nostr:${versionA}\nEarlier conversation context: nostr:${addressB}`
  },
  {
    ...base,
    recipients: [
      {
        public_key:
          '44900586091b284416a0c001f677f9c49f7639a55c3f1e2ec130a8e1a7998e1b',
        relay_url: null
      }
    ],
    reply_to: { id: '2'.repeat(64), relays: null },
    subject: 'Another listing, same ordinary message fields'
  }
];
export const qualifiedMessageReplay = qualifiedMessages.map(
  (expected, index) => {
    const wire_parts = messageToWireParts(JSON.stringify(expected));
    if (!wire_parts || !messageFromWireParts(JSON.stringify(wire_parts)))
      throw new Error(`Unsupported qualified Message recipe ${index}`);
    return {
      id: `qualified_message_${index.toString().padStart(3, '0')}`,
      wire_parts,
      expected
    };
  }
);
