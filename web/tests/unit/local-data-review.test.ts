import { describe, expect, it } from 'vitest';
import {
  createLocalDataReview,
  localDataReviewSnapshot,
  reviewLocalData,
  previewLocalDataRemoval,
  confirmLocalDataRemoval,
  closeLocalDataReview
} from '../../src/lib/persistence/local-data-review.ts';
import { createIdentityViewContext } from '../../src/lib/runtime/view-context.ts';

describe('local data review without original browser account ownership', () => {
  it('construction and SSR review expose no owner or inventory and start no effects', async () => {
    const context = createIdentityViewContext();
    const review = createLocalDataReview(context);
    expect(localDataReviewSnapshot(review)).toEqual({ state: 'guest' });
    await reviewLocalData(review, 'reviewed_local_inventory');
    expect(localDataReviewSnapshot(review)).toEqual({ state: 'guest' });
    expect(await previewLocalDataRemoval(review, 'public', '[]')).toBe(false);
    expect(
      await confirmLocalDataRemoval(review, 'reviewed_local_removal')
    ).toBe(false);
    closeLocalDataReview(review);
    expect(localDataReviewSnapshot(review)).toEqual({ state: 'closed' });
  });
  it('copied and invented handles cannot acquire inventory or deletion authority', async () => {
    const genuine = createLocalDataReview(createIdentityViewContext());
    const copied = { ...genuine };
    expect(localDataReviewSnapshot(copied)).toEqual({ state: 'closed' });
    await reviewLocalData(copied, 'reviewed_local_inventory');
    expect(
      await previewLocalDataRemoval(
        copied,
        'received',
        '["' + 'a'.repeat(64) + '"]'
      )
    ).toBe(false);
    expect(
      await confirmLocalDataRemoval(copied, 'reviewed_local_removal')
    ).toBe(false);
    expect(localDataReviewSnapshot(genuine)).toEqual({ state: 'guest' });
    closeLocalDataReview(genuine);
  });
});
