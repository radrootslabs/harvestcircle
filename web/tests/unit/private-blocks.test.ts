import { expect, test } from 'vitest';
import {
  createLocalPrivateBlocks,
  localPrivateBlocksViewSnapshot,
  closeLocalPrivateBlocksView,
  localPrivateBlocksSnapshot,
  localPrivatePeerAllowed,
  blockLocalPrivatePeer,
  undoLocalPrivatePeer,
  type LocalPrivateBlocks
} from '../../src/lib/persistence/local-private-blocks.ts';
import { createIdentityViewContext } from '../../src/lib/runtime/view-context.ts';
import type { AdmittedConversation } from '../../src/lib/messaging/admit-conversation.ts';
import type { UnlockedSession } from '../../src/lib/messaging/unlocked-session.ts';

test('SSR private preferences expose no account and acquire no browser storage', () => {
  const view = createLocalPrivateBlocks(createIdentityViewContext());
  expect(localPrivateBlocksViewSnapshot(view)).toEqual({ state: 'guest' });
  closeLocalPrivateBlocksView(view);
});

for (const copied of [undefined, null, {}, { state: 'ready', peers: [] }])
  test(`copied private preference token has no custody: ${JSON.stringify(copied)}`, async () => {
    const token = copied as LocalPrivateBlocks;
    expect(localPrivateBlocksSnapshot(token)).toEqual({ state: 'unavailable' });
    expect(localPrivatePeerAllowed(token, 'a'.repeat(64))).toBe(false);
    expect(
      await blockLocalPrivatePeer(
        token,
        {} as AdmittedConversation,
        {} as UnlockedSession,
        'reviewed_local_private_block'
      )
    ).toEqual({ status: 'invalid' });
    expect(
      await undoLocalPrivatePeer(
        token,
        'a'.repeat(64),
        'reviewed_local_private_undo'
      )
    ).toEqual({ status: 'invalid' });
  });
