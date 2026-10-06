import {
  buildFoodTemplate,
  type FoodDraft
} from '../../src/lib/contracts/food-availability-v1/write.ts';
import { foodUnits } from '../../src/lib/contracts/food-availability-v1/values.ts';
const base: FoodDraft = {
  identifier: 'writer-fixed-id',
  title: 'Fresh food',
  description: 'Fresh food available locally.',
  location: 'Victoria',
  published_at: 1700000000,
  created_at: 1700000060,
  amount: '9999999999999999999999999999',
  currency: 'CAD',
  unit: 'g',
  quantity: '10',
  status: 'active'
};
const drafts: readonly FoodDraft[] = [
  ...foodUnits.map((unit) => ({ ...base, unit, identifier: `writer-${unit}` })),
  {
    ...base,
    identifier: 'writer-free-unspecified',
    amount: '0',
    quantity: undefined
  },
  { ...base, identifier: 'writer-sold', status: 'sold' },
  {
    ...base,
    identifier: 'writer-unicode',
    title: '🥕',
    description: '🥕'.repeat(241),
    contact: {
      type: 'https',
      value: 'https://farm.example/contact',
      public: true
    }
  },
  {
    ...base,
    identifier: 'writer-email',
    contact: { type: 'email', value: 'seller@example.com', public: true }
  },
  {
    ...base,
    identifier: 'writer-phone',
    contact: { type: 'phone', value: '+12505550123', public: true }
  },
  { ...base, identifier: 'writer-max-content', description: 'x'.repeat(16384) }
];
export const webTemplates = drafts.map((draft, index) => {
  const result = buildFoodTemplate(draft);
  if (!result.ok) throw new Error(result.error.code);
  return {
    id: `web_writer_${index.toString().padStart(3, '0')}`,
    created_at: draft.created_at,
    wire_parts: result.wire_parts,
    summary: result.summary
  };
});
