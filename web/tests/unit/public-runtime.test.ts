import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
let runtime: typeof import('../../src/lib/runtime/public-runtime.ts');
beforeEach(async () => {
  vi.resetModules();
  runtime = await import('../../src/lib/runtime/public-runtime.ts');
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
describe('root public runtime context', () => {
  it('reserves the view generation across synchronous run-creation clock reentry', () => {
    vi.stubGlobal('window', {});
    const context = runtime.createPublicRuntimeContext();
    let armed = false;
    let inner: ReturnType<typeof runtime.beginPublicViewRun> | undefined;
    const owner = runtime.mountPublicRuntime(context, undefined, {
      now: () => {
        if (armed) {
          armed = false;
          inner = runtime.beginPublicViewRun(view);
        }
        return 0;
      },
      schedule: () => () => {}
    })!;
    const view = runtime.createPublicView(owner);
    armed = true;
    try {
      expect(() => runtime.beginPublicViewRun(view)).toThrow(
        'public_view_run_superseded'
      );
      expect(inner).toBeDefined();
      expect(runtime.publicViewRunCurrent(view, inner!)).toBe(true);
    } finally {
      runtime.closePublicRuntime(context);
    }
  });
  it('creates an inert SSR context without acquiring browser capabilities', () => {
    vi.stubGlobal('window', undefined);
    const context = runtime.createPublicRuntimeContext();
    expect(Object.isFrozen(context)).toBe(true);
    expect(Object.keys(context)).toEqual([]);
    expect(runtime.publicRuntime(context)).toBeUndefined();
    expect(runtime.mountPublicRuntime(context)).toBeUndefined();
    runtime.closePublicRuntime(context);
    expect(runtime.publicRuntime(context)).toBeUndefined();
  });
  it('owns one anonymous mounted browser lifetime and terminal teardown', () => {
    vi.stubGlobal('window', {});
    const context = runtime.createPublicRuntimeContext(),
      owner = runtime.mountPublicRuntime(context)!;
    expect(owner).toBeDefined();
    expect(runtime.mountPublicRuntime(context)).toBe(owner);
    expect(runtime.publicRuntime(context)).toBe(owner);
    const a = runtime.createPublicView(owner),
      b = runtime.createPublicView(owner);
    const run = runtime.beginPublicViewRun(a);
    runtime.beginPublicViewRun(b);
    const next = runtime.beginPublicViewRun(a);
    expect(runtime.publicViewRunCurrent(a, run)).toBe(false);
    expect(runtime.publicViewRunCurrent(a, next)).toBe(true);
    runtime.disposePublicView(a);
    expect(runtime.publicViewRunCurrent(a, next)).toBe(false);
    runtime.closePublicRuntime(context);
    expect(() => runtime.createPublicView(owner)).toThrow(
      'public_runtime_closed'
    );
    expect(() => runtime.mountPublicRuntime(context)).toThrow(
      'public_runtime_closed'
    );
    runtime.closePublicRuntime(context);
  });
  it('rejects forged context and view ownership', () => {
    expect(() =>
      runtime.publicRuntime(
        {} as ReturnType<typeof runtime.createPublicRuntimeContext>
      )
    ).toThrow('public_runtime_context_invalid');
    vi.stubGlobal('window', {});
    const context = runtime.createPublicRuntimeContext(),
      owner = runtime.mountPublicRuntime(context)!;
    expect(owner).toBeDefined();
    expect(() =>
      runtime.beginPublicViewRun(
        {} as ReturnType<typeof runtime.createPublicView>
      )
    ).toThrow('public_view_invalid');
    runtime.closePublicRuntime(context);
    expect(runtime.publicRuntime(context)).toBeUndefined();
  });
  it('settles readiness safely when an SSR context is disposed before hydration', async () => {
    vi.stubGlobal('window', undefined);
    const context = runtime.createPublicRuntimeContext();
    const ready = runtime.publicRuntimeReady(context);
    runtime.closePublicRuntime(context);
    expect(await ready).toBeUndefined();
    expect(() => runtime.mountPublicRuntime(context)).toThrow(
      'public_runtime_closed'
    );
  });
  it('prevents a second root owner or changed manifest/clock from resetting admission', () => {
    vi.stubGlobal('window', {});
    const context = runtime.createPublicRuntimeContext(),
      owner = runtime.mountPublicRuntime(context)!;
    expect(owner).toBeDefined();
    const other = runtime.createPublicRuntimeContext();
    expect(() => runtime.mountPublicRuntime(other)).toThrow(
      'public_runtime_owner_changed'
    );
    const policy = validateRelayPolicy(
      JSON.stringify({
        schemaVersion: 1,
        public: [],
        inbox: [],
        postingEnabled: false,
        messagingEnabled: false,
        operatorDenylist: []
      })
    )!;
    expect(() => runtime.mountPublicRuntime(context, policy)).toThrow(
      'public_runtime_policy_changed'
    );
    expect(() =>
      runtime.mountPublicRuntime(context, undefined, {
        now: () => 0,
        schedule: () => () => {}
      })
    ).toThrow('public_runtime_clock_changed');
    runtime.closePublicRuntime(context);
    runtime.closePublicRuntime(other);
  });
  it('keeps root teardown terminal while a failed actual SDK close remains retryable', async () => {
    const { RelayPool } = await import('applesauce-relay/pool'); // eslint-disable-next-line @typescript-eslint/unbound-method -- Explicit receiver is provided by original.call(this) below.
    const original = RelayPool.prototype.close;
    let calls = 0;
    vi.spyOn(RelayPool.prototype, 'close').mockImplementation(function (
      this: InstanceType<typeof RelayPool>
    ) {
      calls++;
      if (calls === 1) throw new Error('RAW_SDK_CLOSE_ERROR');
      original.call(this);
    });
    vi.stubGlobal('window', {});
    const context = runtime.createPublicRuntimeContext(),
      owner = runtime.mountPublicRuntime(context)!;
    expect(() => runtime.closePublicRuntime(context)).toThrow(
      'public_runtime_close_failed'
    );
    expect(runtime.publicRuntime(context)).toBeUndefined();
    expect(() => runtime.createPublicView(owner)).toThrow(
      'public_runtime_closed'
    );
    runtime.closePublicRuntime(context);
    expect(calls).toBe(2);
  });
});
