import { afterAll, expect, it } from 'vitest';
import { finalizeEvent } from 'applesauce-core/helpers';
import { decodePointer } from 'applesauce-core/helpers/pointers';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../../src/lib/nostr/verified-envelope.ts';
import { enquiryReferences } from '../../src/lib/nostr/enquiry-references.ts';
import {
  captureEnquiryContext,
  enquiryContextSnapshot,
  buildEnquiryMessage,
  assessEnquiryCitation,
  type EnquiryContext
} from '../../src/lib/messaging/enquiry-context.ts';
const keys: Uint8Array[] = [];
afterAll(() => {
  for (const key of keys) key.fill(0);
});
function seller() {
  const key = crypto.getRandomValues(new Uint8Array(32));
  keys.push(key);
  return (
    time = 1700000060,
    title = 'Carrots',
    identifier = 'carrots',
    kind = 30402,
    status = 'active'
  ) => {
    const event = finalizeEvent(
      {
        kind,
        created_at: time,
        content: 'Fresh food',
        tags: [
          ['d', identifier],
          ['title', title],
          ['summary', 'Fresh carrots'],
          ['published_at', '1700000000'],
          ['location', 'Victoria'],
          ['price', '3.5', 'CAD'],
          ['radroots:price_unit', 'lb'],
          ['status', status]
        ]
      },
      key
    );
    const value = verifyEnvelope(JSON.stringify(event));
    if (!value.ok) throw Error('signed public test fixture invalid');
    return value.value;
  };
}
function context(proof: VerifiedEnvelope) {
  const value = captureEnquiryContext(proof);
  if (!value) throw Error('expected verified Food context');
  return value;
}
it('captures genuine signed seller, readable canonical product and exact version without relay hints', () => {
  const proof = seller()(),
    event = verifiedEnvelopeSnapshot(proof)!,
    value = context(proof),
    snapshot = enquiryContextSnapshot(value)!;
  expect(snapshot.peer).toBe(event.pubkey);
  expect(snapshot.eventId).toBe(event.id);
  expect(snapshot.title).toBe('Carrots');
  const address = decodePointer(snapshot.naddr),
    version = decodePointer(snapshot.nevent);
  expect(address).toMatchObject({
    type: 'naddr',
    data: {
      kind: 30402,
      pubkey: event.pubkey,
      identifier: 'carrots',
      relays: []
    }
  });
  expect(version).toMatchObject({
    type: 'nevent',
    data: { id: event.id, author: event.pubkey, kind: 30402, relays: [] }
  });
  const message = buildEnquiryMessage(value, 'Could I collect some tomorrow?')!;
  expect(message.tags).toEqual([['p', event.pubkey]]);
  expect(message.content).toContain('"Carrots"');
  expect(message.content).toContain('nostr:' + snapshot.naddr);
  expect(message.content).toContain('nostr:' + snapshot.nevent);
  expect(message.content).toContain(
    'Advertised price (seller assertion): 3.5 CAD/lb'
  );
  expect(message.content).toContain(
    'This is a message, not a confirmed order.'
  );
  expect(message.content).toContain('Could I collect some tomorrow?');
  expect(message.kind).toBe(14);
  expect(message).not.toHaveProperty('sig');
  expect(message).not.toHaveProperty('id');
});
it('body contact, display names and detached projections cannot redirect the verified publisher', () => {
  const proof = seller()(),
    other = verifiedEnvelopeSnapshot(seller()())!,
    value = context(proof),
    original = enquiryContextSnapshot(value)!;
  const altered = enquiryContextSnapshot(value)! as {
    peer: string;
    title: string;
    eventId: string;
  };
  altered.peer = other.pubkey;
  altered.title = 'Other farm';
  altered.eventId = other.id;
  const message = buildEnquiryMessage(
    value,
    'Reply to nostr:' + other.pubkey + '\nSeller name: Other farm'
  )!;
  expect(message.tags).toEqual([['p', original.peer]]);
  expect(enquiryContextSnapshot(value)).toEqual(original);
  expect(message.content).toContain(original.nevent);
  expect(message.tags.some((tag) => tag[0] === 'e')).toBe(false);
});
it('forged proof/context casts and unsupported private kinds grant no citation or enquiry', () => {
  expect(captureEnquiryContext({} as VerifiedEnvelope)).toBeUndefined();
  expect(enquiryReferences({} as VerifiedEnvelope)).toBeUndefined();
  expect(enquiryContextSnapshot({} as EnquiryContext)).toBeUndefined();
  expect(buildEnquiryMessage({} as EnquiryContext, 'hello')).toBeUndefined();
  for (const kind of [14, 13, 1059, 0, 1]) {
    const proof = seller()(1700000060, 'Carrots', 'carrots', kind);
    expect(enquiryReferences(proof)).toBeUndefined();
    expect(captureEnquiryContext(proof)).toBeUndefined();
  }
});
it('missing or mismatching cited history remains unresolved even with matching body or name', () => {
  const signed = seller(),
    proof = signed(),
    value = context(proof);
  expect(assessEnquiryCitation(value, undefined)).toEqual({
    outcome: 'unresolved',
    reason: 'missing_history'
  });
  expect(assessEnquiryCitation(value, signed(1700000061))).toEqual({
    outcome: 'unresolved',
    reason: 'different_version'
  });
  expect(assessEnquiryCitation(value, seller()())).toEqual({
    outcome: 'unresolved',
    reason: 'different_publisher'
  });
  expect(assessEnquiryCitation(value, {} as VerifiedEnvelope)).toEqual({
    outcome: 'unresolved',
    reason: 'missing_history'
  });
  expect(assessEnquiryCitation(value, proof)).toEqual({
    outcome: 'matched_signed_advertisement',
    peer: verifiedEnvelopeSnapshot(proof)!.pubkey,
    eventId: verifiedEnvelopeSnapshot(proof)!.id
  });
});
it('quoted facts remain advertisement assertions without allocation or locked price state', () => {
  const value = context(seller()()),
    snapshot = enquiryContextSnapshot(value)!;
  expect(snapshot.claims).toBe('signed_advertisement_assertions');
  expect(snapshot).not.toHaveProperty('reserved');
  expect(snapshot).not.toHaveProperty('acceptedOrder');
  expect(
    buildEnquiryMessage(value, 'I would like ten; this text is my assertion')
      ?.tags
  ).toHaveLength(1);
});
it('raw identifier bytes survive canonical references and exact viewed-version capture', () => {
  for (const identifier of ['caf\u00e9', 'cafe\u0301']) {
    const proof = seller()(1700000060, 'Carrots', identifier),
      refs = enquiryReferences(proof)!,
      value = context(proof);
    expect(refs.identifier).toBe(identifier);
    expect(enquiryContextSnapshot(value)?.identifier).toBe(identifier);
    expect(buildEnquiryMessage(value, '')?.content).toContain(
      'nostr:' + refs.naddr
    );
  }
});
it('generic BOM reference preservation does not admit an unsupported Food identifier', () => {
  const proof = seller()(1700000060, 'Carrots', '\ufeffcarrots');
  expect(enquiryReferences(proof)?.identifier).toBe('\ufeffcarrots');
  expect(captureEnquiryContext(proof)).toBeUndefined();
});
it('composed body includes context within the four KiB limit and never truncates', () => {
  const value = context(seller()()),
    seed = buildEnquiryMessage(value, '')!.content,
    remaining = 4096 - new TextEncoder().encode(seed).length - 2;
  expect(
    new TextEncoder().encode(
      buildEnquiryMessage(value, 'a'.repeat(remaining))!.content
    ).length
  ).toBe(4096);
  expect(buildEnquiryMessage(value, 'a'.repeat(remaining + 1))).toBeUndefined();
  expect(
    buildEnquiryMessage(value, 'é'.repeat(Math.floor(remaining / 2)))
  ).toBeDefined();
  expect(buildEnquiryMessage(value, '\ud800')).toBeUndefined();
  expect(buildEnquiryMessage(value, { redirect: 'other' })).toBeUndefined();
});
it('known sold listing remains citable but cannot create a new product enquiry', () => {
  const proof = seller()(1700000060, 'Carrots', 'carrots', 30402, 'sold'),
    value = context(proof);
  expect(enquiryContextSnapshot(value)?.status).toBe('sold');
  expect(assessEnquiryCitation(value, proof)?.outcome).toBe(
    'matched_signed_advertisement'
  );
  expect(buildEnquiryMessage(value, 'Could I buy some?')).toBeUndefined();
});
