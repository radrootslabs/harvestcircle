import { describe, it, expect } from 'vitest';
import {
  createIdentityViewContext,
  identityViewSnapshot,
  mountIdentityView,
  connectIdentityView,
  probeIdentityViewMessaging,
  disconnectIdentityView,
  closeIdentityView,
  subscribeIdentityView,
  identityViewMatchesOwner,
  type IdentityViewContext
} from '../../src/lib/runtime/view-context.ts';
describe('genuine per-client identity view context', () => {
  it('constructs separate inert disconnected contexts without browser acquisition', () => {
    const a = createIdentityViewContext(),
      b = createIdentityViewContext();
    expect(a).not.toBe(b);
    expect(Object.keys(a)).toEqual([]);
    expect(identityViewSnapshot(a)).toEqual({
      mounted: false,
      identity: { state: 'guest', reason: 'disconnected' }
    });
    disconnectIdentityView(a);
    expect(identityViewSnapshot(b).identity).toEqual({
      state: 'guest',
      reason: 'disconnected'
    });
  });
  it('rejects forged tokens and does not infer authorization from a public key', async () => {
    const fake = {} as IdentityViewContext;
    expect(mountIdentityView(fake)).toBe(false);
    expect(identityViewMatchesOwner(fake, 'a'.repeat(64))).toBe(false);
    expect((await connectIdentityView(fake)).identity.state).toBe('guest');
  });
  it('SSR cannot mount, connect or invoke a reviewed messaging action', async () => {
    const c = createIdentityViewContext();
    expect(mountIdentityView(c)).toBe(false);
    expect((await connectIdentityView(c)).identity.state).toBe('guest');
    expect(
      (await probeIdentityViewMessaging(c, 'reviewed_self_copy')).identity.state
    ).toBe('guest');
  });
  it('snapshot mutation and one subscriber cannot change another context', () => {
    const a = createIdentityViewContext(),
      b = createIdentityViewContext();
    const events: string[] = [];
    const off = subscribeIdentityView(a, (state) =>
      events.push(state.identity.state)
    );
    const copy = identityViewSnapshot(a);
    (copy as { mounted: boolean }).mounted = true;
    expect(identityViewSnapshot(a).mounted).toBe(false);
    closeIdentityView(a);
    off();
    expect(events.length).toBeGreaterThan(0);
    expect(identityViewSnapshot(b).mounted).toBe(false);
    expect(identityViewMatchesOwner(b, 'a'.repeat(64))).toBe(false);
  });
});
