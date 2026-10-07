import { afterEach, expect, it, vi } from 'vitest';
import { finalizeEvent, getEventHash } from 'applesauce-core/helpers';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import {
  captureEnquiryContext,
  enquiryContextSnapshot,
  type EnquiryContext
} from '../../src/lib/messaging/enquiry-context.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity,
  type IdentitySession
} from '../../src/lib/runtime/identity-session.ts';
import {
  captureEnquiryRumor,
  captureReplyRumor,
  rumorPlanSnapshot,
  rumorEnvelopePlan,
  stopRumorPlan,
  type RumorPlan
} from '../../src/lib/messaging/rumor-plan.ts';
const buyer =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
  outsider = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
const sessions: IdentitySession[] = [];
afterEach(() => {
  for (const session of sessions) disconnectIdentity(session);
  sessions.length = 0;
  vi.unstubAllGlobals();
});
function food() {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const event = finalizeEvent(
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
    const proof = verifyEnvelope(JSON.stringify(event));
    if (!proof.ok) throw Error('invalid public test fixture');
    const context = captureEnquiryContext(proof.value);
    if (!context) throw Error('invalid Food context');
    return { event, context };
  } finally {
    key.fill(0);
  }
}
async function identity(owner = buyer) {
  let encrypts = 0,
    decrypts = 0,
    signs = 0;
  vi.stubGlobal('window', {
    nostr: {
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
  const session = createIdentitySession();
  sessions.push(session);
  await connectIdentity(session);
  await probeIdentityMessaging(session, 'reviewed_self_copy');
  return { session, counts: () => ({ encrypts, decrypts, signs }) };
}
function required(value: RumorPlan | undefined) {
  if (!value) throw Error('expected local rumor plan');
  return value;
}
it('binds current observed owner and genuine Food seller without signing/encrypting or persisting', async () => {
  const f = food(),
    owner = await identity(),
    before = owner.counts();
  const plan = required(
    captureEnquiryRumor(owner.session, f.context, 'Tomorrow?', 1700000100)
  );
  const snapshot = rumorPlanSnapshot(plan)!,
    wire = JSON.parse(snapshot.wire) as {
      pubkey: string;
      tags: string[][];
      id: string;
      kind: number;
      created_at: number;
      content: string;
    };
  expect(snapshot.owner).toBe(buyer);
  expect(snapshot.peer).toBe(f.event.pubkey);
  expect(wire.pubkey).toBe(buyer);
  expect(wire.tags).toEqual([['p', f.event.pubkey]]);
  expect(wire.id).toBe(getEventHash(wire));
  expect(wire).not.toHaveProperty('sig');
  expect(wire.created_at).toBe(1700000100);
  expect(owner.counts()).toEqual(before);
});
it('exactly peer and self destinations share one immutable rumor including its original p', async () => {
  const f = food(),
    owner = await identity(),
    plan = required(captureEnquiryRumor(owner.session, f.context, '', 100));
  const copies = rumorEnvelopePlan(plan)!;
  expect(copies.map((row) => [row.role, row.recipient])).toEqual([
    ['peer', f.event.pubkey],
    ['self_archive', buyer]
  ]);
  expect(copies[0].rumorWire).toBe(copies[1].rumorWire);
  for (const row of copies)
    expect((JSON.parse(row.rumorWire) as { tags: string[][] }).tags).toEqual([
      ['p', f.event.pubkey]
    ]);
  const detached = copies as { recipient: string; rumorWire: string }[];
  detached[0].recipient = outsider;
  detached[0].rumorWire = '{}';
  expect(rumorEnvelopePlan(plan)![0].recipient).toBe(f.event.pubkey);
  expect(rumorEnvelopePlan(plan)![0].rumorWire).toBe(copies[1].rumorWire);
});
it('body and detached display/context cannot redirect actual recipient', async () => {
  const f = food(),
    owner = await identity(),
    copy = enquiryContextSnapshot(f.context)! as { peer: string };
  copy.peer = outsider;
  const plan = required(
    captureEnquiryRumor(owner.session, f.context, 'Send to ' + outsider, 100)
  );
  expect(rumorPlanSnapshot(plan)!.peer).toBe(f.event.pubkey);
  expect(
    captureEnquiryRumor(
      owner.session,
      copy as unknown as EnquiryContext,
      '',
      100
    )
  ).toBeUndefined();
});
it('rejects own listing, guest/forged identity, forged context and invalid time', async () => {
  const f = food(),
    seller = await identity(f.event.pubkey);
  expect(
    captureEnquiryRumor(seller.session, f.context, '', 100)
  ).toBeUndefined();
  const guest = createIdentitySession();
  sessions.push(guest);
  expect(captureEnquiryRumor(guest, f.context, '', 100)).toBeUndefined();
  expect(
    captureEnquiryRumor({} as IdentitySession, f.context, '', 100)
  ).toBeUndefined();
  const owner = await identity();
  expect(
    captureEnquiryRumor(owner.session, {} as EnquiryContext, '', 100)
  ).toBeUndefined();
  expect(captureEnquiryRumor(owner.session, f.context, '', -0)).toBeUndefined();
});
it('reply derives opposite participant and direct rumor parent, never the public listing event', async () => {
  const f = food(),
    owner = await identity(),
    parent = required(captureEnquiryRumor(owner.session, f.context, '', 100));
  const parentId = rumorPlanSnapshot(parent)!.id,
    seller = await identity(f.event.pubkey);
  const reply = required(
      captureReplyRumor(seller.session, parent, 'Yes, tomorrow works.', 101)
    ),
    snapshot = rumorPlanSnapshot(reply)!;
  const wire = JSON.parse(snapshot.wire) as {
    pubkey: string;
    tags: string[][];
  };
  expect(snapshot.owner).toBe(f.event.pubkey);
  expect(snapshot.peer).toBe(buyer);
  expect(wire.pubkey).toBe(f.event.pubkey);
  expect(wire.tags).toEqual([
    ['p', buyer],
    ['e', parentId]
  ]);
  expect(parentId).not.toBe(f.event.id);
  expect(rumorEnvelopePlan(reply)!.map((row) => row.recipient)).toEqual([
    buyer,
    f.event.pubkey
  ]);
});
it('same-owner followup keeps the opposite peer and rejects nonmember or injected parent IDs', async () => {
  const f = food(),
    owner = await identity(),
    parent = required(captureEnquiryRumor(owner.session, f.context, '', 100));
  const followup = required(
    captureReplyRumor(owner.session, parent, 'Another question', 101)
  );
  expect(rumorPlanSnapshot(followup)!.peer).toBe(f.event.pubkey);
  expect(
    captureReplyRumor(
      owner.session,
      f.event.id as unknown as RumorPlan,
      'Injected parent',
      101
    )
  ).toBeUndefined();
  expect(
    captureReplyRumor(
      owner.session,
      rumorPlanSnapshot(parent) as unknown as RumorPlan,
      'Detached parent',
      101
    )
  ).toBeUndefined();
  const other = await identity(outsider);
  expect(
    captureReplyRumor(other.session, parent, 'Not a room member', 101)
  ).toBeUndefined();
});
it('detached plan data and forged token cannot change or acquire plaintext ownership', async () => {
  const f = food(),
    owner = await identity(),
    plan = required(captureEnquiryRumor(owner.session, f.context, '', 100));
  const before = rumorPlanSnapshot(plan)!;
  const copy = rumorPlanSnapshot(plan)! as {
    owner: string;
    peer: string;
    wire: string;
  };
  copy.owner = outsider;
  copy.peer = buyer;
  copy.wire = '{}';
  expect(rumorPlanSnapshot(plan)).toEqual(before);
  expect(rumorPlanSnapshot({} as RumorPlan)).toBeUndefined();
  expect(rumorEnvelopePlan(copy as unknown as RumorPlan)).toBeUndefined();
});
it('explicit release and identity invalidation erase reachable plan plaintext and reject further replies', async () => {
  const f = food(),
    owner = await identity(),
    first = required(captureEnquiryRumor(owner.session, f.context, '', 100)),
    second = required(
      captureEnquiryRumor(owner.session, f.context, 'Other', 101)
    );
  stopRumorPlan(first);
  stopRumorPlan(first);
  expect(rumorPlanSnapshot(first)).toBeUndefined();
  expect(rumorEnvelopePlan(first)).toBeUndefined();
  disconnectIdentity(owner.session);
  expect(rumorPlanSnapshot(second)).toBeUndefined();
  expect(rumorEnvelopePlan(second)).toBeUndefined();
  expect(
    captureReplyRumor(owner.session, second, 'After disconnect', 102)
  ).toBeUndefined();
});
it('reply body objects cannot execute conversion or become trusted plain text', async () => {
  const f = food(),
    owner = await identity();
  const parent = required(
    captureEnquiryRumor(owner.session, f.context, '', 100)
  );
  const toJSON = vi.fn(() => 'Object-supplied private body');
  expect(
    captureReplyRumor(owner.session, parent, { toJSON }, 101)
  ).toBeUndefined();
  expect(toJSON).not.toHaveBeenCalled();
});
