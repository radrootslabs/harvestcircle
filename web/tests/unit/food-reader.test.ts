import { describe, expect, it } from 'vitest';
import { markerPartition } from '../../src/lib/contracts/food-availability-v1/partition.ts';
import {
  projectFoodParts,
  readFoodEvent
} from '../../src/lib/contracts/food-availability-v1/read.ts';
import { incomingAmount } from '../../src/lib/contracts/food-availability-v1/text.ts';
import { projectImages } from '../../src/lib/contracts/food-availability-v1/media.ts';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';

const base = {
  kind: 30402,
  created_at: 1700000060,
  content: 'Food this week.',
  tags: [
    ['d', 'carrots'],
    ['title', 'Carrots'],
    ['summary', 'Fresh carrots'],
    ['published_at', '1700000000'],
    ['location', 'Victoria'],
    ['price', '003.500', 'cad'],
    ['radroots:price_unit', 'lb'],
    ['status', 'active']
  ]
};
function changed(name: string, value: string[]) {
  return {
    ...base,
    tags: base.tags.map((tag) => (tag[0] === name ? value : [...tag]))
  };
}
function code(value: ReturnType<typeof projectFoodParts>) {
  return value.outcome === 'rejected' ? value.error.code : value.outcome;
}
describe('raw marker partition precedes shape and focused-domain checks', () => {
  for (const name of ['radroots:price_unit', 'radroots:quantity'])
    it(name, () =>
      expect(markerPartition([[name]])).toBe('focused_food_availability')
    );
  for (const name of ['radroots:primary_bin', 'radroots:bin', 'radroots:price'])
    it(name, () => {
      expect(markerPartition([[name]])).toBe('operational_listing');
      expect(
        code(projectFoodParts({ ...base, tags: [[name], ['delivery']] }))
      ).toBe('excluded');
      expect(
        code(
          projectFoodParts({ ...base, tags: [[name], ['radroots:quantity']] })
        )
      ).toBe('food_profile_ambiguous');
    });
  it('exact names only and empty tags', () =>
    expect(
      markerPartition([[], ['Radroots:quantity'], ['radroots:quantity ']])
    ).toBe('generic_nip99'));
  it('unknown core fields never prevent generic exclusion', () =>
    expect(
      code(
        projectFoodParts({
          ...base,
          tags: [
            ['status', 'unknown'],
            ['price', 'bad']
          ]
        })
      )
    ).toBe('excluded'));
});
describe('tolerant values retain precision and unknown versus zero', () => {
  for (const [input, expected] of [
    ['000.000', '0'],
    ['003.500', '3.5'],
    ['0000000000000000000000000001', '1'],
    ['9999999999999999999999999999', '9999999999999999999999999999']
  ])
    it(input, () => expect(incomingAmount(input)).toBe(expected));
  for (const input of [
    '',
    '.1',
    '1.',
    '+1',
    '-1',
    '1e3',
    '1,000',
    ' 1',
    '١',
    '0'.repeat(29),
    '1.2.3'
  ])
    it('rejects ' + input, () => expect(incomingAmount(input)).toBeUndefined());
  it('normalizes inbound only', () => {
    const r = projectFoodParts(base);
    expect(r.outcome).toBe('focused');
    if (r.outcome === 'focused') {
      expect(r.projection.price).toEqual({
        amount: '3.5',
        currency: 'CAD',
        unit: 'lb'
      });
      expect(r.projection.quantity).toBeNull();
    }
  });
  for (const status of ['withdrawn', 'ACTIVE', 'unknown', ''])
    it('unknown status ' + status, () =>
      expect(
        code(projectFoodParts(changed('status', ['status', status])))
      ).toBe('food_status_invalid')
    );
  it('zero price is valid while quantity zero is not missing', () => {
    expect(
      code(projectFoodParts(changed('price', ['price', '0', 'CAD'])))
    ).toBe('focused');
    expect(
      code(
        projectFoodParts({
          ...base,
          tags: [...base.tags, ['radroots:quantity', '000.0', 'lb']]
        })
      )
    ).toBe('quantity_zero');
  });
  it('bad quantity cannot become zero or missing', () =>
    expect(
      code(
        projectFoodParts({
          ...base,
          tags: [...base.tags, ['radroots:quantity', 'bad', 'lb']]
        })
      )
    ).toBe('quantity_invalid'));
  it('singleton shape checks precede invalid content', () =>
    expect(
      code(
        projectFoodParts({
          ...base,
          content: '',
          tags: [...base.tags, ['title', 'duplicate']]
        })
      )
    ).toBe('food_tag_invalid'));
  it('unsafe numeric time and unsupported publication time fail', () => {
    expect(
      code(
        projectFoodParts({
          ...base,
          created_at: Number('18446744073709551615')
        })
      )
    ).toBe('envelope_invalid');
    expect(
      code(
        projectFoodParts(
          changed('published_at', ['published_at', '18446744073709551615'])
        )
      )
    ).toBe('food_published_at_invalid');
  });
  for (const value of [
    '\ufeffCarrots',
    'Carrots\u200b',
    'Carrots\n',
    ' Carrots',
    'Carrots\u0085'
  ])
    it('rejects invalid text ' + JSON.stringify(value), () =>
      expect(code(projectFoodParts(changed('title', ['title', value])))).toBe(
        'food_text_invalid'
      )
    );
  it('identifier is byte bounded without imposing text trimming', () =>
    expect(code(projectFoodParts(changed('d', ['d', 'é'.repeat(257)])))).toBe(
      'food_identifier_invalid'
    ));
});
describe('bounded incoming image diagnostics are structural only', () => {
  const hash = 'a'.repeat(64);
  it('first64 and ordered overflow diagnosis', () => {
    const r = projectImages(
      Array.from({ length: 65 }, (_, i) => [
        'image',
        `https://media.example/${i.toString(16).padStart(64, '0')}.webp`,
        '800x600'
      ])
    );
    expect(r.images).toHaveLength(64);
    expect(r.diagnostics).toEqual(['food_image_count_exceeded']);
  });
  it('raw duplicates and digest duplicates differ', () => {
    const r = projectImages([
      ['image', `https://a.example/${hash}.png`, '1x1'],
      ['image', `https://b.example/${hash}.webp`, '1x1'],
      ['image', 'bad', '01x1'],
      ['image', 'bad']
    ]);
    expect(r.images[1].diagnostics).toEqual(['food_image_duplicate_digest']);
    expect(r.images[3].diagnostics).toEqual([
      'food_image_shape_invalid',
      'food_image_url_invalid',
      'food_image_dimensions_missing',
      'food_image_duplicate_url'
    ]);
  });
  for (const url of [
    'https://user:pass@a.example/a',
    'https://a.example',
    'https://é.example/a',
    'https://a..example/a',
    'javascript:alert(1)',
    'https://a.example/\u200ba'
  ])
    it('rejects URL ' + url, () =>
      expect(projectImages([['image', url, '1x1']]).images[0].url).toBeNull()
    );
  for (const dims of ['0x1', '01x1', '1X1', '4294967296x1', '1x1x1'])
    it('rejects dimensions ' + dims, () =>
      expect(
        projectImages([['image', 'https://a.example/a', dims]]).images[0]
          .dimensions
      ).toBeNull()
    );
  it('accepts full u32 dimensions', () =>
    expect(
      projectImages([['image', 'https://a.example/a', '4294967295x1']])
        .images[0].dimensions
    ).toEqual({ width: 4294967295, height: 1 }));
});
describe('raw input has no implied verification trust', () => {
  for (const raw of [
    undefined,
    {},
    'null',
    '[]',
    '{',
    ' '.repeat(262145),
    '"\\ud800"'
  ])
    it('rejects malformed bounded envelope', () =>
      expect(verifyEnvelope(raw).ok).toBe(false));
  it('invalid signature is rejected before generic/operational classification', () =>
    expect(
      readFoodEvent(
        JSON.stringify({
          id: '0'.repeat(64),
          pubkey: '0'.repeat(64),
          sig: '0'.repeat(128),
          created_at: 1,
          kind: 30402,
          content: '',
          tags: [['radroots:bin']]
        })
      ).outcome
    ).toBe('rejected'));
});

describe('exact shared reader text and numeric boundaries', () => {
  it('content maximum uses UTF8 bytes and never prototype form limits', () => {
    expect(
      code(projectFoodParts({ ...base, content: 'a'.repeat(131072) }))
    ).toBe('focused');
    expect(
      code(projectFoodParts({ ...base, content: 'é'.repeat(65537) }))
    ).toBe('food_content_too_large');
  });
  it('all contract whitespace is missing while internal content controls are preserved', () => {
    expect(
      code(projectFoodParts({ ...base, content: '\u001c\u001f\u0085' }))
    ).toBe('food_content_missing');
    expect(code(projectFoodParts({ ...base, content: 'a\u0000b' }))).toBe(
      'focused'
    );
  });
  it('text byte limit and identifiers do not use display character counts', () => {
    expect(
      code(projectFoodParts(changed('title', ['title', 'é'.repeat(2048)])))
    ).toBe('focused');
    expect(
      code(projectFoodParts(changed('title', ['title', 'é'.repeat(2049)])))
    ).toBe('envelope_invalid');
    expect(code(projectFoodParts(changed('d', ['d', 'é'.repeat(256)])))).toBe(
      'focused'
    );
  });
  for (const input of ['0', '01', '+1', '1e3', '1700000061'])
    it('publication ' + input, () =>
      expect(
        code(projectFoodParts(changed('published_at', ['published_at', input])))
      ).toBe(
        input === '1700000061'
          ? 'food_published_at_future'
          : 'food_published_at_invalid'
      )
    );
  for (const input of ['cad', 'cAd', 'AAA'])
    it('actual inbound ASCII currency ' + input, () =>
      expect(
        code(projectFoodParts(changed('price', ['price', '1', input])))
      ).toBe('focused')
    );
  for (const input of ['CA', 'ＣAD', '12A', 'CA D'])
    it('reject currency ' + input, () =>
      expect(
        code(projectFoodParts(changed('price', ['price', '1', input])))
      ).toBe('price_currency_invalid')
    );
  it('unit values are exact and quantity does not normalize case or mismatched units', () => {
    expect(
      code(
        projectFoodParts(
          changed('radroots:price_unit', ['radroots:price_unit', 'LB'])
        )
      )
    ).toBe('price_unit_invalid');
    expect(
      code(
        projectFoodParts({
          ...base,
          tags: [...base.tags, ['radroots:quantity', '1', 'kg']]
        })
      )
    ).toBe('quantity_invalid');
  });
  it('duplicate quantity is invalid even when both match', () =>
    expect(
      code(
        projectFoodParts({
          ...base,
          tags: [
            ...base.tags,
            ['radroots:quantity', '1', 'lb'],
            ['radroots:quantity', '1', 'lb']
          ]
        })
      )
    ).toBe('quantity_invalid'));
  it('ill formed Unicode cannot pass through UTF8 replacement', () => {
    expect(code(projectFoodParts(changed('d', ['d', '\ud800'])))).toBe(
      'envelope_invalid'
    );
    expect(code(projectFoodParts(changed('title', ['title', '\ud800'])))).toBe(
      'envelope_invalid'
    );
  });
});

it('bounded constant errors cannot be altered through a prior rejection', () => {
  const prior = verifyEnvelope('null');
  if (!prior.ok)
    expect(() => {
      (prior.error as { message: string }).message = 'unbounded replacement';
    }).toThrow();
  const next = verifyEnvelope('null');
  if (!next.ok)
    expect(next.error).toEqual({
      code: 'envelope_invalid',
      message: 'invalid or unsupported bounded NIP-01 envelope'
    });
});

describe('direct pure parts remain bounded before marker/value scans', () => {
  it('tag count limit is enforced before generic exclusion', () =>
    expect(
      code(
        projectFoodParts({
          ...base,
          tags: Array.from({ length: 1025 }, () => ['t', 'food'])
        })
      )
    ).toBe('envelope_invalid'));
  it('raw content work cap is separate from shared content limit', () =>
    expect(
      code(projectFoodParts({ ...base, content: 'x'.repeat(262145) }))
    ).toBe('envelope_invalid'));
  it('tag byte and total element limits precede profile checks', () => {
    expect(
      code(projectFoodParts({ ...base, tags: [['t', 'é'.repeat(2049)]] }))
    ).toBe('envelope_invalid');
    expect(
      code(
        projectFoodParts({
          ...base,
          tags: [['t', ...Array.from({ length: 4096 }, () => 'x')]]
        })
      )
    ).toBe('envelope_invalid');
  });
});
