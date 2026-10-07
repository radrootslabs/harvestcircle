import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createIdentitySession,
  captureIdentityPublicOperation,
  identityPublicOperationOwnership,
  invalidateIdentityOperations,
  reviewIdentityLatePublicArtifact,
  type IdentityPublicOperation
} from '../../src/lib/runtime/identity-session.ts';
await test('late-result/session modules remain inert and forged SSR operations have no capability', () => {
  assert.equal(typeof window, 'undefined');
  const session = createIdentitySession();
  const fake = {} as IdentityPublicOperation;
  assert.equal(
    captureIdentityPublicOperation(
      session,
      {} as never,
      '',
      'reviewed_captured_operation'
    ),
    undefined
  );
  assert.equal(identityPublicOperationOwnership(session, fake), undefined);
  invalidateIdentityOperations(session);
  assert.equal(
    reviewIdentityLatePublicArtifact(
      session,
      fake,
      {} as never,
      'review_original_late_artifact'
    ),
    undefined
  );
});
