import { expect, test } from 'vitest';
import {
  openSellingView,
  sellingViewSnapshot,
  closeSellingView
} from '../../src/lib/publishing/selling-view.ts';
import type { IdentityViewContext } from '../../src/lib/runtime/view-context.ts';
import type { SellingView } from '../../src/lib/publishing/selling-view.ts';

for (const context of [undefined, null, {}, { owner: 'copied' }])
  test(`seller index cannot acquire account storage from ${JSON.stringify(context)}`, async () => {
    const view = await openSellingView(context as IdentityViewContext);
    expect(view).toBeUndefined();
  });

test('copied seller presentation token has no original snapshot or retirement authority', () => {
  const copied = {} as SellingView;
  expect(sellingViewSnapshot(copied)).toBeUndefined();
  expect(() => closeSellingView(copied)).not.toThrow();
});
