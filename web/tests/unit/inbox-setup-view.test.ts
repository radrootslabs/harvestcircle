import { inboxResolverCurrentAfterSample } from '../../src/lib/messaging/resolve-inbox.ts';
import { it, expect } from 'vitest';
import {
  createInboxSetupView,
  checkInboxSetupView,
  reviewInboxSetupView,
  inboxSetupViewSnapshot,
  stopInboxSetupView,
  closeInboxSetupView,
  subscribeInboxSetupView,
  unlockInboxSetupView
} from '../../src/lib/messaging/inbox-setup-view.ts';
import {
  makeSetupFixture,
  fixturePolicy,
  inbox
} from '../e2e/harness/inbox-setup.ts';
it('incomplete actual bounded lookup remains unknown, never global absence', async () => {
  const f = await makeSetupFixture();
  const view = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(f.resolve(null, false).resolver),
    now: () => 101
  });
  try {
    await checkInboxSetupView(view);
    const state = inboxSetupViewSnapshot(view);
    expect(state.status).toBe('lookup_incomplete');
    expect(state.currentWire).toBeNull();
    expect(state.setupComplete).toBe(false);
    expect(f.counts().signs).toBe(0);
  } finally {
    closeInboxSetupView(view);
    f.close();
  }
});
it('exact review preserves unknown entries and displays global effect without signing', async () => {
  const f = await makeSetupFixture();
  const view = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(f.resolve().resolver),
    now: () => 101
  });
  try {
    await checkInboxSetupView(view);
    await reviewInboxSetupView(view, [inbox], [], []);
    const state = inboxSetupViewSnapshot(view);
    expect(state.status).toBe('review');
    expect(state.preview?.globalEffect).toContain('other clients');
    expect(state.preview?.selectedInboxes).toEqual([inbox]);
    expect(
      (JSON.parse(state.preview!.wire) as { tags: string[][] }).tags
    ).toEqual([...f.event.tags, ['relay', inbox]]);
    expect(f.counts().signs).toBe(0);
    stopInboxSetupView(view);
    expect(inboxSetupViewSnapshot(view).preview).toBeUndefined();
    expect(inboxSetupViewSnapshot(view).status).toBe('paused');
  } finally {
    closeInboxSetupView(view);
    f.close();
  }
});
it('compatible observed configuration offers Unlock without replacing a global preference', async () => {
  const f = await makeSetupFixture([['relay', inbox]]);
  const view = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(f.resolve().resolver),
    now: () => 101
  });
  try {
    await checkInboxSetupView(view);
    expect(inboxSetupViewSnapshot(view).status).toBe('compatible');
    await reviewInboxSetupView(view, [inbox], [], []);
    expect(inboxSetupViewSnapshot(view).preview).toBeUndefined();
    expect(f.counts().signs).toBe(0);
    expect(inboxSetupViewSnapshot(view).setupComplete).toBe(false);
  } finally {
    closeInboxSetupView(view);
    f.close();
  }
});
it('late original lookup cannot restore a stopped panel', async () => {
  const f = await makeSetupFixture();
  let release: (() => void) | undefined;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const view = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: async () => {
      await gate;
      return f.resolve().resolver;
    }
  });
  try {
    const pending = checkInboxSetupView(view);
    stopInboxSetupView(view);
    release?.();
    await pending;
    expect(inboxSetupViewSnapshot(view).status).toBe('paused');
    expect(inboxSetupViewSnapshot(view).setupComplete).toBe(false);
    expect(f.counts().signs).toBe(0);
  } finally {
    closeInboxSetupView(view);
    f.close();
  }
});

it('synchronous checking cancellation prevents lookup from starting', async () => {
  const f = await makeSetupFixture();
  let lookups = 0;
  const view = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => {
      lookups++;
      return Promise.resolve(f.resolve().resolver);
    }
  });
  const unsubscribe = subscribeInboxSetupView(view, (state) => {
    if (state.status === 'checking' && state.busy) stopInboxSetupView(view);
  });
  try {
    await checkInboxSetupView(view);
    expect(lookups).toBe(0);
    expect(inboxSetupViewSnapshot(view).status).toBe('paused');
  } finally {
    unsubscribe();
    closeInboxSetupView(view);
    f.close();
  }
});
it('synchronous approval cancellation prevents a fresh provider observation', async () => {
  const f = await makeSetupFixture();
  const view = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(f.resolve().resolver)
  });
  const unsubscribe = subscribeInboxSetupView(view, (state) => {
    if (state.status === 'approval_wait' && state.busy)
      stopInboxSetupView(view);
  });
  try {
    const keys = f.counts().keys;
    await unlockInboxSetupView(view);
    expect(f.counts().keys).toBe(keys);
    expect(inboxSetupViewSnapshot(view).status).toBe('paused');
  } finally {
    unsubscribe();
    closeInboxSetupView(view);
    f.close();
  }
});
it('Not now closes a retained incomplete discovery resolver', async () => {
  const f = await makeSetupFixture();
  const retained = f.resolve(null, false).resolver;
  const view = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(retained)
  });
  try {
    await checkInboxSetupView(view);
    expect(inboxResolverCurrentAfterSample(retained)).toBe(true);
    stopInboxSetupView(view);
    expect(inboxResolverCurrentAfterSample(retained)).toBe(false);
    expect(inboxSetupViewSnapshot(view).status).toBe('paused');
  } finally {
    closeInboxSetupView(view);
    f.close();
  }
});

it('closing is terminal before notifying observers', async () => {
  const f = await makeSetupFixture();
  let lookups = 0;
  let notifications = 0;
  let restarted: Promise<void> | undefined;
  const view = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => {
      lookups++;
      return Promise.resolve(f.resolve().resolver);
    }
  });
  const unsubscribe = subscribeInboxSetupView(view, (state) => {
    notifications++;
    if (state.status === 'paused' && !state.busy)
      restarted = checkInboxSetupView(view);
  });
  try {
    closeInboxSetupView(view);
    await restarted;
    expect(lookups).toBe(0);
    expect(inboxSetupViewSnapshot(view).status).toBe('closed');
    const settledNotifications = notifications;
    closeInboxSetupView(view);
    expect(notifications).toBe(settledNotifications);
  } finally {
    unsubscribe();
    f.close();
  }
});

it('closing prevents provider observation from a teardown observer', async () => {
  const f = await makeSetupFixture();
  let restarted: Promise<void> | undefined;
  const view = createInboxSetupView({
    identity: f.identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(f.resolve().resolver)
  });
  const unsubscribe = subscribeInboxSetupView(view, (state) => {
    if (state.status === 'paused' && !state.busy)
      restarted = unlockInboxSetupView(view);
  });
  try {
    const keys = f.counts().keys;
    closeInboxSetupView(view);
    await restarted;
    expect(f.counts().keys).toBe(keys);
    expect(inboxSetupViewSnapshot(view).status).toBe('closed');
  } finally {
    unsubscribe();
    f.close();
  }
});
