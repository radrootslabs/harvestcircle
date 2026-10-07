import { expect, it } from 'vitest';
import { createIdentitySession } from '../../src/lib/runtime/identity-session.ts';
import {
  createPrivateSession,
  privateSessionSnapshot,
  privateSessionOwnership,
  closePrivateSession,
  type PrivateSession
} from '../../src/lib/runtime/private-session.ts';
import { getPrivatePool } from '../../src/lib/nostr/private-pool.ts';
it('SSR private import and explicit admission acquire no browser transport', async () => {
  const identity = createIdentitySession();
  expect(
    await createPrivateSession(identity, 'reviewed_private_session')
  ).toBeUndefined();
  expect(getPrivatePool({} as PrivateSession, {} as never, [])).toBeUndefined();
});
it('guest observation and unreviewed admission do not create private authority', async () => {
  const identity = createIdentitySession();
  expect(await createPrivateSession(identity, 'unreviewed')).toBeUndefined();
  expect(
    await createPrivateSession({} as never, 'reviewed_private_session')
  ).toBeUndefined();
});
it('forged private session cannot expose an owner or generation', () => {
  expect(privateSessionSnapshot({} as PrivateSession)).toBeUndefined();
  expect(privateSessionOwnership({} as PrivateSession)).toBeUndefined();
  expect(() => closePrivateSession({} as PrivateSession)).not.toThrow();
});
