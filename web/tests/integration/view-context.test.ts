import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createIdentityViewContext,
  mountIdentityView,
  connectIdentityView,
  closeIdentityView,
  identityViewSnapshot
} from '../../src/lib/runtime/view-context.ts';
await test('root identity view construction and explicit SSR calls acquire no browser identity', async () => {
  assert.equal(typeof window, 'undefined');
  const c = createIdentityViewContext();
  assert.equal(mountIdentityView(c), false);
  assert.equal((await connectIdentityView(c)).mounted, false);
  closeIdentityView(c);
  assert.equal(identityViewSnapshot(c).identity.state, 'guest');
});
