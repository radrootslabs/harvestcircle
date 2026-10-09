import { describe, expect, it } from 'vitest';
import {
  captureCitedProduct,
  citedProductSnapshot,
  closeCitedProduct,
  conversationOptionsSnapshot,
  requestConversationOption,
  type CitedProduct
} from '../../src/lib/messaging/cited-product.ts';
import type { InboxView } from '../../src/lib/messaging/inbox-view.ts';
import type { PublicRuntime } from '../../src/lib/runtime/public-runtime.ts';

const id = '12345678-1234-4234-8234-123456789abc';
describe('private citation and conversation options reject detached authority', () => {
  it('SSR cannot allocate a citation query from arbitrary scalar hints', () => {
    expect(
      captureCitedProduct(
        {} as InboxView,
        id,
        'a'.repeat(64),
        'naddr1invalid',
        {} as PublicRuntime,
        'reviewed_product_citation'
      )
    ).toBeUndefined();
  });
  it('copied citation tokens do not expose verified context', () => {
    expect(citedProductSnapshot({} as CitedProduct)).toBeUndefined();
  });
  it('closing a fake citation never creates query authority', () => {
    const fake = {} as CitedProduct;
    closeCitedProduct(fake);
    expect(citedProductSnapshot(fake)).toBeUndefined();
  });
  it('fake views do not expose an authenticated conversation identity', () => {
    expect(conversationOptionsSnapshot({} as InboxView, id)).toBeUndefined();
  });
  it('fake local options cannot invoke a block or private report hook', () => {
    const peers: string[] = [];
    for (const action of ['block', 'report'] as const)
      expect(
        requestConversationOption(
          {} as InboxView,
          id,
          action,
          (peer) => peers.push(peer),
          'reviewed_conversation_option'
        )
      ).toBe(false);
    expect(peers).toEqual([]);
  });
  it('unreviewed citation capture cannot become an effect', () => {
    expect(
      captureCitedProduct(
        {} as InboxView,
        id,
        'a'.repeat(64),
        'naddr1invalid',
        {} as PublicRuntime,
        undefined
      )
    ).toBeUndefined();
  });
});
