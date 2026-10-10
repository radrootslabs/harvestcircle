import { expect, it } from 'vitest';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../src/lib/runtime/identity-session.ts';
import {
  createInboxSetupView,
  checkInboxSetupView,
  unlockInboxSetupView,
  stopInboxSetupView,
  closeInboxSetupView
} from '../../src/lib/messaging/inbox-setup-view.ts';
import {
  makeSetupFixture,
  fixturePolicy,
  inbox
} from '../e2e/harness/inbox-setup.ts';
import {
  newListingReadinessSnapshot,
  captureNewListingReadiness,
  newListingReadinessOwnership,
  closeNewListingReadiness,
  type NewListingReadiness
} from '../../src/lib/publishing/new-listing-readiness.ts';
import type { ObserveInboxAccess } from '../../src/lib/messaging/readiness.ts';

// HC_TEST_ONLY_QUALIFICATION_PORT: exercises the original owner-bound gate,
// not real extension/relay access, retention or launch qualification.
async function fixture(access = true) {
  const f = await makeSetupFixture([['relay', inbox]]);
  let currentAccess = true;
  const observeAccess: ObserveInboxAccess = (context) => ({
    ...context,
    readTargets: context.readTargets.slice(),
    archiveTargets: context.archiveTargets.slice(),
    receive: 'qualified_exercised',
    archive: 'qualified_exercised',
    current: () => currentAccess
  });
  const setup = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(f.resolve().resolver),
    ...(access ? { observeAccess } : {})
  });
  if (!setup) throw Error('original setup unavailable');
  await checkInboxSetupView(setup);
  await unlockInboxSetupView(setup);
  return {
    ...f,
    setup,
    retireAccess: () => {
      currentAccess = false;
    },
    close() {
      closeInboxSetupView(setup);
      f.close();
    }
  };
}
const review = 'reviewed_new_listing_inbox';
it('guest can draft but cannot review a new reply-capable listing', () => {
  const identity = createIdentitySession();
  expect(newListingReadinessSnapshot(identity, undefined)).toMatchObject({
    draft: true,
    readyToReview: false,
    preferenceWrite: 'not_requested'
  });
  expect(
    captureNewListingReadiness(identity, undefined, review)
  ).toBeUndefined();
});
it('actual signing-only identity cannot advertise new messaging-ready publication', async () => {
  const f = await makeSetupFixture();
  try {
    disconnectIdentity(f.identity);
    const provider = window.nostr as {
      getPublicKey(): Promise<string>;
      signEvent(...args: unknown[]): Promise<never>;
    };
    Object.defineProperty(window, 'nostr', {
      configurable: true,
      value: {
        getPublicKey: () => provider.getPublicKey(),
        signEvent: (...args: Parameters<typeof provider.signEvent>) =>
          provider.signEvent(...args)
      }
    });
    await connectIdentity(f.identity);
    const before = f.counts();
    expect(
      captureNewListingReadiness(f.identity, undefined, review)
    ).toBeUndefined();
    expect(
      newListingReadinessSnapshot(f.identity, undefined).readyToReview
    ).toBe(false);
    expect(f.counts()).toEqual(before);
  } finally {
    f.close();
  }
});
it('compatible signed configuration without exercised qualified access remains blocked', async () => {
  const f = await fixture(false);
  try {
    const before = f.counts();
    expect(newListingReadinessSnapshot(f.identity, f.setup)).toMatchObject({
      draft: true,
      readyToReview: false,
      preferenceWrite: 'not_requested'
    });
    expect(
      captureNewListingReadiness(f.identity, f.setup, review)
    ).toBeUndefined();
    expect(f.counts()).toEqual(before);
  } finally {
    f.close();
  }
});
it('original qualified compatible inbox permits explicit review without another preference write or signature', async () => {
  const f = await fixture();
  try {
    const before = f.counts();
    expect(newListingReadinessSnapshot(f.identity, f.setup).readyToReview).toBe(
      true
    );
    const proof = captureNewListingReadiness(f.identity, f.setup, review);
    const owned = newListingReadinessOwnership(proof);
    expect(owned?.owner).toBe(f.owner);
    expect(owned?.identity).toBe(f.identity);
    expect(owned?.current()).toBe(true);
    expect(f.counts()).toEqual(before);
    expect(f.counts().signs).toBe(0);
  } finally {
    f.close();
  }
});
for (const mode of ['missing', 'partial', 'unsupported'] as const)
  it(
    'actual signed/bounded own inbox cannot admit new review: ' + mode,
    async () => {
      const f = await makeSetupFixture([
        ['relay', mode === 'unsupported' ? '' : inbox]
      ]);
      const resolver = f.resolve(
        mode === 'missing' ? null : f.event,
        mode !== 'partial'
      ).resolver;
      const setup = createInboxSetupView({
        identity: f.identity,
        policy: fixturePolicy(),
        lookup: () => Promise.resolve(resolver)
      });
      try {
        await checkInboxSetupView(setup);
        await unlockInboxSetupView(setup);
        const before = f.counts();
        expect(
          captureNewListingReadiness(f.identity, setup, review)
        ).toBeUndefined();
        expect(
          newListingReadinessSnapshot(f.identity, setup).readyToReview
        ).toBe(false);
        expect(f.counts()).toEqual(before);
      } finally {
        closeInboxSetupView(setup);
        f.close();
      }
    }
  );
it('copied setup and readiness tokens cannot borrow genuine owner authority', async () => {
  const f = await fixture();
  try {
    const proof = captureNewListingReadiness(f.identity, f.setup, review)!;
    expect(newListingReadinessOwnership(proof)?.current()).toBe(true);
    expect(
      captureNewListingReadiness({ ...f.identity }, f.setup, review)
    ).toBeUndefined();
    expect(
      captureNewListingReadiness(f.identity, { ...f.setup }, review)
    ).toBeUndefined();
    expect(newListingReadinessOwnership({ ...proof })).toBeUndefined();
    expect(
      newListingReadinessOwnership({} as NewListingReadiness)
    ).toBeUndefined();
    expect(newListingReadinessOwnership(proof)?.current()).toBe(true);
  } finally {
    f.close();
  }
});
it('another original identity cannot borrow the seller setup even with the same displayed observations', async () => {
  const f = await fixture(),
    other = createIdentitySession();
  try {
    await connectIdentity(other);
    await probeIdentityMessaging(other, 'reviewed_self_copy');
    expect(captureNewListingReadiness(other, f.setup, review)).toBeUndefined();
    expect(
      captureNewListingReadiness(f.identity, f.setup, review)
    ).toBeDefined();
  } finally {
    disconnectIdentity(other);
    f.close();
  }
});
it('retired actual exercised access revokes held proof without signing or publishing', async () => {
  const f = await fixture();
  try {
    const proof = captureNewListingReadiness(f.identity, f.setup, review)!;
    const held = newListingReadinessOwnership(proof)!;
    expect(held.current()).toBe(true);
    f.retireAccess();
    expect(held.current()).toBe(false);
    expect(newListingReadinessOwnership(proof)).toBeUndefined();
    expect(f.counts().signs).toBe(0);
  } finally {
    f.close();
  }
});
for (const invalidate of ['stop', 'close', 'disconnect', 'revoke'] as const)
  it(
    'original lifecycle invalidation revokes a new-listing review: ' +
      invalidate,
    async () => {
      const f = await fixture();
      try {
        const proof = captureNewListingReadiness(f.identity, f.setup, review)!;
        const held = newListingReadinessOwnership(proof)!;
        expect(held.current()).toBe(true);
        if (invalidate === 'stop') stopInboxSetupView(f.setup);
        if (invalidate === 'close') closeInboxSetupView(f.setup);
        if (invalidate === 'disconnect') disconnectIdentity(f.identity);
        if (invalidate === 'revoke') closeNewListingReadiness(proof);
        expect(held.current()).toBe(false);
        expect(newListingReadinessOwnership(proof)).toBeUndefined();
        expect(f.counts().signs).toBe(0);
      } finally {
        f.close();
      }
    }
  );
it('unreviewed readiness observations cannot silently create a review capability', async () => {
  const f = await fixture();
  try {
    const before = f.counts();
    expect(
      captureNewListingReadiness(f.identity, f.setup, undefined)
    ).toBeUndefined();
    expect(
      captureNewListingReadiness(f.identity, f.setup, true)
    ).toBeUndefined();
    expect(f.counts()).toEqual(before);
  } finally {
    f.close();
  }
});
