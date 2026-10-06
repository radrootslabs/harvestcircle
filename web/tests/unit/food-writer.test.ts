import { describe, expect, it } from 'vitest';
import {
  buildFoodTemplate,
  type FoodDraft
} from '../../src/lib/contracts/food-availability-v1/write.ts';
import { publicContact } from '../../src/lib/contracts/food-availability-v1/contact.ts';
import { foodUnits } from '../../src/lib/contracts/food-availability-v1/values.ts';
const draft: FoodDraft = {
  identifier: 'fixed-draft-id',
  title: 'Carrots',
  description: 'Fresh carrots.\nCollected locally.',
  location: 'Victoria',
  created_at: 1700000060,
  published_at: 1700000000,
  amount: '3.5',
  currency: 'CAD',
  unit: 'lb',
  quantity: '10',
  status: 'active'
};
function valid(value: FoodDraft) {
  const result = buildFoodTemplate(value);
  expect(result.ok).toBe(true);
  if (!result.ok) throw new Error(result.error.code);
  return result;
}
describe('known fields are read once before validation', () => {
  it('title getter cannot replace the validated title', () => {
    let reads = 0;
    const r = valid({
      ...draft,
      get title() {
        return ++reads === 1 ? 'Carrots' : 'x'.repeat(161);
      }
    });
    expect(reads).toBe(1);
    expect(r.wire_parts.tags[1]).toEqual(['title', 'Carrots']);
  });
  it('description getter cannot replace the bounded description', () => {
    let reads = 0;
    const r = valid({
      ...draft,
      get description() {
        return ++reads === 1 ? 'Carrots' : 'x'.repeat(20000);
      }
    });
    expect(reads).toBe(1);
    expect(r.wire_parts.content).toBe('Carrots');
  });
  it('contact getter and its fields cannot replace validated contact', () => {
    let contacts = 0;
    let values = 0;
    const r = valid({
      ...draft,
      get contact() {
        contacts++;
        return {
          type: 'email' as const,
          public: true as const,
          get value() {
            return ++values === 1
              ? 'seller@example.com'
              : 'attacker?body=private@example.com';
          }
        };
      }
    });
    expect(contacts).toBe(1);
    expect(values).toBe(1);
    expect(r.wire_parts.content).toBe(
      draft.description + '\n\nPublic contact: mailto:seller@example.com'
    );
  });
  it('public contact snapshots disclosure and type once', () => {
    let types = 0;
    let disclosures = 0;
    expect(
      publicContact({
        get public() {
          disclosures++;
          return true;
        },
        get type() {
          return ++types === 1 ? 'email' : 'phone';
        },
        value: 'seller@example.com'
      })
    ).toEqual({ type: 'email', href: 'mailto:seller@example.com' });
    expect(types).toBe(1);
    expect(disclosures).toBe(1);
  });
});
describe('strict text-only authored Food templates', () => {
  for (const unit of foodUnits)
    it(unit, () => {
      const r = valid({ ...draft, unit });
      expect(r.wire_parts.kind).toBe(30402);
      expect(r.wire_parts.tags).toEqual([
        ['d', 'fixed-draft-id'],
        ['title', 'Carrots'],
        ['summary', 'Fresh carrots. Collected locally.'],
        ['published_at', '1700000000'],
        ['location', 'Victoria'],
        ['price', '3.5', 'CAD'],
        ['radroots:price_unit', unit],
        ['radroots:quantity', '10', unit],
        ['status', 'active']
      ]);
    });
  it('stable identity/publication time is not inferred from changed title', () => {
    const a = valid(draft);
    const b = valid({
      ...draft,
      title: 'New title',
      created_at: 1700000070,
      status: 'sold'
    });
    expect(a.wire_parts.tags[0]).toEqual(b.wire_parts.tags[0]);
    expect(a.wire_parts.tags[3]).toEqual(b.wire_parts.tags[3]);
  });
  it('summary is derived and visible with whole Unicode code points', () => {
    const r = valid({ ...draft, description: '😀'.repeat(241) });
    expect(r.summary).toBe('😀'.repeat(240));
    expect(r.summary.isWellFormed()).toBe(true);
    expect(r.wire_parts.tags[2]).toEqual(['summary', r.summary]);
  });
  it('fresh draft line endings normalize without changing already signed data', () => {
    const r = valid({ ...draft, description: '  Fresh\r\ncarrots.\rHere.  ' });
    expect(r.wire_parts.content).toBe('Fresh\ncarrots.\nHere.');
    expect(r.summary).toBe('Fresh carrots. Here.');
  });
  it('zero price and absent quantity differ from quantity zero', () => {
    const r = valid({ ...draft, amount: '0', quantity: undefined });
    expect(r.wire_parts.tags.find((t) => t[0] === 'price')).toEqual([
      'price',
      '0',
      'CAD'
    ]);
    expect(r.wire_parts.tags.some((t) => t[0] === 'radroots:quantity')).toBe(
      false
    );
    expect(buildFoodTemplate({ ...draft, quantity: '0' }).ok).toBe(false);
  });
  it('canonical28digit amount remains exact', () =>
    expect(
      valid({
        ...draft,
        amount: '9999999999999999999999999999'
      }).wire_parts.tags.find((t) => t[0] === 'price')?.[1]
    ).toBe('9999999999999999999999999999'));
  for (const amount of ['003.500', '1.0', '+1', '1e3', '1,000', '1'.repeat(29)])
    it('rejects authored amount ' + amount, () =>
      expect(buildFoodTemplate({ ...draft, amount }).ok).toBe(false)
    );
  for (const value of [
    { currency: 'cad' },
    { unit: 'lbs' },
    { quantity: '-1' },
    { identifier: '' },
    { identifier: 'x y' },
    { identifier: 'x'.repeat(513) },
    { status: 'withdrawn' },
    { created_at: Number('18446744073709551615') },
    { published_at: 1700000061 },
    { published_at: 0 },
    { title: ' x' },
    { title: 'x\u200b' },
    { title: '😀'.repeat(161) },
    { location: 'x'.repeat(161) },
    { description: '' },
    { description: 'x'.repeat(16385) }
  ])
    it('rejects invalid draft ' + JSON.stringify(value), () =>
      expect(buildFoodTemplate({ ...draft, ...value } as FoodDraft).ok).toBe(
        false
      )
    );
  it('actual160codepoint titles and locations plus16KiB content are admitted', () => {
    const r = valid({
      ...draft,
      title: '😀'.repeat(160),
      location: 'é'.repeat(160),
      description: 'x'.repeat(16384)
    });
    expect(new TextEncoder().encode(r.wire_parts.content)).toHaveLength(16384);
  });
  it('contact block participates in composed16KiB budget', () =>
    expect(
      buildFoodTemplate({
        ...draft,
        description: 'x'.repeat(16380),
        contact: { type: 'email', value: 'seller@example.com', public: true }
      }).ok
    ).toBe(false));
  it('only explicitly public contact composes into content', () => {
    const r = valid({
      ...draft,
      contact: { type: 'email', value: 'seller@example.com', public: true }
    });
    expect(r.wire_parts.content).toBe(
      draft.description + '\n\nPublic contact: mailto:seller@example.com'
    );
    expect(
      r.wire_parts.tags.some((t) =>
        ['contact', 'image', 'contract'].includes(t[0])
      )
    ).toBe(false);
    expect(
      buildFoodTemplate({
        ...draft,
        contact: {
          type: 'email',
          value: 'seller@example.com',
          public: false
        } as never
      }).ok
    ).toBe(false);
  });
  it('private enquiry/subject/recipient extras never enter the public event', () => {
    const r = valid({
      ...draft,
      privateEnquiry: 'PRIVATE ENQUIRY',
      subject: 'PRIVATE SUBJECT',
      recipient: 'PRIVATE RECIPIENT',
      summary: 'PRIVATE SUMMARY',
      images: ['https://secret.example/image']
    } as FoodDraft);
    expect(JSON.stringify(r.wire_parts)).not.toContain('PRIVATE');
    expect(JSON.stringify(r.wire_parts)).not.toContain('secret.example');
    expect(r.summary).toBe('Fresh carrots. Collected locally.');
  });
});
describe('dedicated optional public-contact fields', () => {
  for (const input of [
    { type: 'email', value: 'seller+food@example.com', public: true },
    { type: 'phone', value: '+12505550123', public: true },
    { type: 'https', value: 'https://farm.example/contact', public: true }
  ])
    it('allowed ' + input.type, () =>
      expect(publicContact(input as never)).toBeDefined()
    );
  for (const input of [
    { type: 'email', value: 'seller@example.com?body=private', public: true },
    { type: 'email', value: 'a\nb@example.com', public: true },
    { type: 'email', value: 'missing-domain', public: true },
    { type: 'phone', value: '555 HELP', public: true },
    { type: 'phone', value: '+0', public: true },
    { type: 'https', value: 'http://farm.example/contact', public: true },
    {
      type: 'https',
      value: 'https://user:pass@farm.example/contact',
      public: true
    },
    { type: 'https', value: 'javascript:alert(1)', public: true },
    { type: 'https', value: 'https://farm.example/%0a', public: true },
    { type: 'https', value: 'https://farm.example/contact', public: false },
    { type: 'other', value: 'private prose', public: true }
  ])
    it('rejects ' + JSON.stringify(input), () =>
      expect(publicContact(input as never)).toBeUndefined()
    );
  it('absence stays absent', () =>
    expect(publicContact(undefined)).toBeUndefined());
});

describe('contact recipient and visible URI preservation', () => {
  for (const value of [
    'foo?subject=x@example.com',
    'foo#bar@example.com',
    'foo%3Fbar@example.com',
    'foo%0abar@example.com'
  ])
    it('reject mailbox URI syntax ' + value, () =>
      expect(
        publicContact({ type: 'email', value, public: true })
      ).toBeUndefined()
    );
  for (const value of [
    'https://farm\u200b.example/contact',
    'https://farm.example/%E2%80%8Bcontact'
  ])
    it('reject URL format character ' + value, () =>
      expect(
        publicContact({ type: 'https', value, public: true })
      ).toBeUndefined()
    );
  it('normal mailbox preserves its exact recipient with no header/fragment', () => {
    const contact = publicContact({
      type: 'email',
      value: 'seller+food@example.com',
      public: true
    })!;
    const parsed = new URL(contact.href);
    expect(parsed.pathname).toBe('seller+food@example.com');
    expect(parsed.search).toBe('');
    expect(parsed.hash).toBe('');
  });
});
