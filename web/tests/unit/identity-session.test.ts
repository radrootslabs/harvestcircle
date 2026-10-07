import { expect, it } from 'vitest';
import {
  createIdentitySession,
  identitySessionSnapshot,
  connectIdentity,
  disconnectIdentity,
  recheckIdentityOwner,
  probeIdentityMessaging,
  captureIdentityPublicOperation,
  identityPublicOperationOwnership,
  invalidateIdentityOperations,
  type IdentityPublicOperation,
  type IdentitySession
} from '../../src/lib/runtime/identity-session.ts';
it('forged or disconnected original operation cannot acquire a session ownership context', () => {
  const session = createIdentitySession();
  expect(
    captureIdentityPublicOperation(
      session,
      {} as never,
      '',
      'reviewed_captured_operation'
    )
  ).toBeUndefined();
  expect(
    identityPublicOperationOwnership(session, {} as IdentityPublicOperation)
  ).toBeUndefined();
  invalidateIdentityOperations(session);
  expect(
    identityPublicOperationOwnership(session, {} as IdentityPublicOperation)
  ).toBeUndefined();
});
it('new session is disconnected guest and exports no signer or key custody', () => {
  const session = createIdentitySession();
  expect(identitySessionSnapshot(session)).toEqual({
    state: 'guest',
    reason: 'disconnected'
  });
  expect(Object.keys(session)).toEqual([]);
});
it('session snapshots are detached and fabricated handles are unavailable', () => {
  const session = createIdentitySession();
  const view = identitySessionSnapshot(session);
  (view as { reason: string }).reason = 'changed';
  expect(identitySessionSnapshot(session)).toEqual({
    state: 'guest',
    reason: 'disconnected'
  });
  expect(identitySessionSnapshot({} as IdentitySession)).toEqual({
    state: 'guest',
    reason: 'unavailable'
  });
});
it('SSR explicit connect stays guest without window or storage', async () => {
  const session = createIdentitySession();
  expect(await connectIdentity(session)).toEqual({
    state: 'guest',
    reason: 'unavailable'
  });
});
it('disconnected owner recheck does not infer remembered authority', async () => {
  const session = createIdentitySession();
  expect(await recheckIdentityOwner(session)).toEqual({
    state: 'guest',
    reason: 'disconnected'
  });
});
it('unreviewed encryption probe refuses before any capability action', async () => {
  const session = createIdentitySession();
  expect(await probeIdentityMessaging(session, 'unreviewed')).toEqual({
    state: 'guest',
    reason: 'disconnected'
  });
});
it('disconnect is idempotent and does not retain a key', () => {
  const session = createIdentitySession();
  disconnectIdentity(session);
  disconnectIdentity(session);
  expect(identitySessionSnapshot(session)).toEqual({
    state: 'guest',
    reason: 'disconnected'
  });
});
