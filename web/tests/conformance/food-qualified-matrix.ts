import {
  buildFoodTemplate,
  type FoodDraft
} from '../../src/lib/contracts/food-availability-v1/write.ts';

// Browser-owned boundary recipes, separate from the original shared manifest.
// Actual Rust consumes these unsigned templates; no signature or key is stored.
const base: FoodDraft = {
  identifier: 'qualified-base',
  title: 'Fresh food',
  description: 'Food available locally.',
  location: 'Victoria',
  published_at: 1700000000,
  created_at: 1700000060,
  amount: '1',
  currency: 'CAD',
  unit: 'kg',
  quantity: '1',
  status: 'active'
};
const minimum = '0.' + '0'.repeat(26) + '1';
export const qualifiedDrafts: readonly FoodDraft[] = [
  { ...base, identifier: 'qualified-smallest-price', amount: minimum },
  {
    ...base,
    identifier: 'qualified-mixed-price',
    amount: '12345678901234.56789012345678'
  },
  {
    ...base,
    identifier: 'qualified-largest-quantity',
    quantity: '9999999999999999999999999999'
  },
  { ...base, identifier: 'qualified-smallest-quantity', quantity: minimum },
  {
    ...base,
    identifier: 'qualified-free-unknown-quantity',
    amount: '0',
    quantity: undefined
  },
  {
    ...base,
    identifier: 'qualified-max-safe-clock',
    created_at: Number.MAX_SAFE_INTEGER,
    published_at: Number.MAX_SAFE_INTEGER
  },
  { ...base, identifier: 'qualified-unicode-title', title: '🥕'.repeat(160) },
  {
    ...base,
    identifier: 'qualified-unicode-location',
    location: '漢'.repeat(160)
  },
  {
    ...base,
    identifier: 'qualified-unicode-content',
    description: '🥕'.repeat(4096)
  },
  { ...base, identifier: 'qualified-é' },
  { ...base, identifier: 'qualified-e\u0301' },
  { ...base, identifier: 'Qualified:Case:ID' },
  {
    ...base,
    identifier: 'qualified-sold-unknown-quantity',
    status: 'sold',
    quantity: undefined
  },
  {
    ...base,
    identifier: 'qualified-normalized-lines',
    description: '  Fresh 🥕\r\nAvailable today.\rLocally.  '
  },
  {
    ...base,
    identifier: 'qualified-earliest-clock',
    published_at: 1,
    created_at: 1
  },
  {
    ...base,
    identifier: 'qualified-largest-price-mixed-quantity',
    amount: '9999999999999999999999999999',
    quantity: '12345678901234.56789012345678',
    unit: 'basket',
    status: 'sold'
  }
];
export const qualifiedTemplates = qualifiedDrafts.map((draft, index) => {
  const built = buildFoodTemplate(draft);
  if (!built.ok) throw new Error(`${draft.identifier}: ${built.error.code}`);
  return {
    id: `qualified_writer_${index.toString().padStart(3, '0')}`,
    created_at: draft.created_at,
    wire_parts: built.wire_parts,
    summary: built.summary
  };
});
