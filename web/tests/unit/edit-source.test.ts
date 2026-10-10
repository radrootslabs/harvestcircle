import { expect, it } from 'vitest';
import { render } from 'svelte/server';
import ConflictNotice from '../../src/lib/components/ConflictNotice.svelte';
import {
  openOwnedEditSource,
  ownedEditSourceSnapshot,
  authorizeOwnedEditSource,
  closeOwnedEditSource,
  type OwnedEditSource
} from '../../src/lib/publishing/edit-source.ts';
import type { PublicPublicationRepository } from '../../src/lib/persistence/public-publications.ts';
import type { IdentityViewContext } from '../../src/lib/runtime/view-context.ts';
it('HCP118 copied repositories and unmounted identities cannot adopt a remote source', async () => {
  expect(
    await openOwnedEditSource(
      {} as PublicPublicationRepository,
      {} as IdentityViewContext,
      'untrusted'
    )
  ).toBeUndefined();
});
it('HCP118 copied ready snapshots cannot authorize editing or supply current/proposed authority', () => {
  const copied = {
    status: 'ready',
    owner: '0'.repeat(64)
  } as unknown as OwnedEditSource;
  expect(ownedEditSourceSnapshot(copied)).toBeUndefined();
  expect(authorizeOwnedEditSource(copied)).toBeUndefined();
  expect(
    render(ConflictNotice, { props: { source: copied } }).body
  ).not.toContain('This listing changed elsewhere');
});
it('HCP118 closing copied or absent custody never manufactures readiness', () => {
  const copied = {} as OwnedEditSource;
  closeOwnedEditSource(copied);
  expect(authorizeOwnedEditSource(copied)).toBeUndefined();
});
