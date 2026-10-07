import { expect, it } from 'vitest';
import {
  createExtensionScheduler,
  browserExtensionScheduler,
  runExtensionAction,
  callExtension,
  stopExtensionAction,
  markExtensionWaitExpired,
  extensionSchedulerSnapshot,
  type ExtensionAction,
  type ExtensionScheduler
} from '../../src/lib/nostr/extension-scheduler.ts';
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
function capture() {
  return { owner, session: Symbol(), operation: Symbol() };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
it('SSR construction is inert and no browser-global scheduler is created', () => {
  expect(browserExtensionScheduler()).toBeUndefined();
  expect(extensionSchedulerSnapshot(createExtensionScheduler())).toEqual({
    state: 'idle'
  });
});
it('sign/decrypt overlap reports busy without a call or automatic retry', async () => {
  const scheduler = createExtensionScheduler(),
    gate = deferred<string>();
  let calls = 0;
  const first = runExtensionAction(
    scheduler,
    capture(),
    () => true,
    (a) =>
      callExtension(a, 'sign', () => {
        calls++;
        return gate.promise;
      })
  );
  expect(
    await runExtensionAction(
      scheduler,
      capture(),
      () => true,
      (a) =>
        callExtension(a, 'decrypt', () => {
          calls++;
          return Promise.resolve('text');
        })
    )
  ).toEqual({ status: 'busy' });
  expect(calls).toBe(1);
  gate.resolve('signature');
  await first;
  expect(calls).toBe(1);
  expect(
    (
      await runExtensionAction(
        scheduler,
        capture(),
        () => true,
        (a) =>
          callExtension(a, 'decrypt', () => {
            calls++;
            return Promise.resolve('text');
          })
      )
    ).status
  ).toBe('completed');
  expect(calls).toBe(2);
});
it('denial pauses the original batch across decrypt/sign/encrypt/AUTH', async () => {
  const scheduler = createExtensionScheduler();
  let calls = 0;
  const result = await runExtensionAction(
    scheduler,
    capture(),
    () => true,
    async (a) => {
      expect(
        await callExtension(a, 'decrypt', () => {
          calls++;
          return Promise.reject(new Error('private refusal prose'));
        })
      ).toEqual({ status: 'denied' });
      for (const kind of ['sign', 'encrypt', 'auth'] as const)
        expect(
          await callExtension(a, kind, () => {
            calls++;
            return Promise.resolve('unused');
          })
        ).toEqual({ status: 'denied' });
    }
  );
  expect(result.status).toBe('denied');
  expect(calls).toBe(1);
  expect(JSON.stringify(result)).not.toContain('private refusal prose');
});
it('wait expiration preserves a late original result and holds the pending slot', async () => {
  const scheduler = createExtensionScheduler(),
    gate = deferred<string>(),
    original = capture();
  let action!: ExtensionAction;
  const task = runExtensionAction(
    scheduler,
    original,
    () => true,
    (a) => {
      action = a;
      return callExtension(a, 'sign', () => gate.promise);
    }
  );
  markExtensionWaitExpired(action);
  expect(extensionSchedulerSnapshot(scheduler)).toMatchObject({
    state: 'active',
    pending: 'sign',
    phase: 'wait_expired'
  });
  expect(
    await runExtensionAction(
      scheduler,
      capture(),
      () => true,
      () => Promise.resolve('unexpected')
    )
  ).toEqual({ status: 'busy' });
  gate.resolve('late signature');
  const result = await task;
  expect(result).toMatchObject({
    status: 'wait_expired',
    capture: original,
    value: {
      status: 'settled',
      current: false,
      value: 'late signature',
      capture: original
    }
  });
  expect(extensionSchedulerSnapshot(scheduler)).toEqual({ state: 'idle' });
});
it('stop retains a forgotten await and rejects further effects until actual settlement', async () => {
  const scheduler = createExtensionScheduler(),
    gate = deferred<string>();
  let action!: ExtensionAction,
    settled = false;
  const task = runExtensionAction(
    scheduler,
    capture(),
    () => true,
    (a) => {
      action = a;
      void callExtension(a, 'key', () => gate.promise);
      return Promise.resolve('work returned');
    }
  ).then((v) => {
    settled = true;
    return v;
  });
  stopExtensionAction(action);
  await Promise.resolve();
  await Promise.resolve();
  expect(settled).toBe(false);
  expect(
    await runExtensionAction(
      scheduler,
      capture(),
      () => true,
      () => Promise.resolve('unused')
    )
  ).toEqual({ status: 'busy' });
  expect(
    await callExtension(action, 'auth', () => Promise.resolve('unused'))
  ).toEqual({ status: 'stopped' });
  gate.resolve('late key');
  expect((await task).status).toBe('stopped');
});
it('within-action parallel calls cannot overlap, and sequential calls retain capture', async () => {
  const scheduler = createExtensionScheduler(),
    gate = deferred<string>();
  let calls = 0;
  await runExtensionAction(
    scheduler,
    capture(),
    () => true,
    async (a) => {
      const first = callExtension(a, 'encrypt', () => {
        calls++;
        return gate.promise;
      });
      expect(
        await callExtension(a, 'decrypt', () => {
          calls++;
          return Promise.resolve('unused');
        })
      ).toEqual({ status: 'busy' });
      gate.resolve('cipher');
      expect((await first).status).toBe('settled');
      expect(
        (
          await callExtension(a, 'decrypt', () => {
            calls++;
            return Promise.resolve('plain');
          })
        ).status
      ).toBe('settled');
    }
  );
  expect(calls).toBe(2);
});
it('freshness invalidation prevents new effects and marks admitted late results stale', async () => {
  const scheduler = createExtensionScheduler(),
    gate = deferred<string>();
  let current = true,
    calls = 0;
  const task = runExtensionAction(
    scheduler,
    capture(),
    () => current,
    async (a) => {
      const value = await callExtension(a, 'key', () => gate.promise);
      expect(
        await callExtension(a, 'auth', () => {
          calls++;
          return Promise.resolve('unused');
        })
      ).toEqual({ status: 'stopped' });
      return value;
    }
  );
  current = false;
  gate.resolve(owner);
  expect(await task).toMatchObject({
    status: 'stopped',
    value: { current: false, value: owner }
  });
  expect(calls).toBe(0);
});
it('fabricated handles and invalid captures have zero effects; snapshots are detached', async () => {
  let calls = 0;
  const scheduler = createExtensionScheduler();
  expect(
    await runExtensionAction(
      {} as ExtensionScheduler,
      capture(),
      () => true,
      () => {
        calls++;
        return Promise.resolve();
      }
    )
  ).toEqual({ status: 'invalid' });
  expect(
    await runExtensionAction(
      scheduler,
      { ...capture(), owner: 'invalid' },
      () => true,
      () => {
        calls++;
        return Promise.resolve();
      }
    )
  ).toEqual({ status: 'invalid' });
  expect(
    await callExtension({} as ExtensionAction, 'key', () => {
      calls++;
      return Promise.resolve();
    })
  ).toEqual({ status: 'invalid' });
  const snapshot = extensionSchedulerSnapshot(scheduler);
  (snapshot as { state: string }).state = 'changed';
  expect(extensionSchedulerSnapshot(scheduler)).toEqual({ state: 'idle' });
  expect(calls).toBe(0);
});
it('reentrant freshness callback cannot admit a competing action', async () => {
  const scheduler = createExtensionScheduler();
  let competitor: Promise<unknown> | undefined,
    calls = 0;
  await runExtensionAction(
    scheduler,
    capture(),
    () => {
      competitor ??= runExtensionAction(
        scheduler,
        capture(),
        () => true,
        () => {
          calls++;
          return Promise.resolve();
        }
      );
      return true;
    },
    (a) => callExtension(a, 'key', () => Promise.resolve(owner))
  );
  expect(await competitor).toEqual({ status: 'busy' });
  expect(calls).toBe(0);
});
it('input and returned capture mutation cannot change the retained original', async () => {
  const original = capture(),
    expected = { ...original };
  const result = await runExtensionAction(
    createExtensionScheduler(),
    original,
    () => true,
    async (a) => {
      original.owner = 'changed';
      const first = await callExtension(a, 'key', () => Promise.resolve(owner));
      if (first.status === 'settled')
        (first.capture as { owner: string }).owner = 'changed again';
      return callExtension(a, 'sign', () => Promise.resolve('signature'));
    }
  );
  expect(result).toMatchObject({
    capture: expected,
    value: { capture: expected }
  });
});
it('undefined owner is invalid even with genuine session and operation symbols', async () => {
  let calls = 0;
  expect(
    await runExtensionAction(
      createExtensionScheduler(),
      { ...capture(), owner: undefined } as never,
      () => true,
      () => {
        calls++;
        return Promise.resolve();
      }
    )
  ).toEqual({ status: 'invalid' });
  expect(calls).toBe(0);
});
it('completed work cannot schedule another call through a leaked action while forgotten work settles', async () => {
  const scheduler = createExtensionScheduler(),
    gate = deferred<string>();
  let action!: ExtensionAction,
    calls = 0;
  const task = runExtensionAction(
    scheduler,
    capture(),
    () => true,
    (a) => {
      action = a;
      void callExtension(a, 'sign', () => gate.promise);
      return Promise.resolve();
    }
  );
  await Promise.resolve();
  await Promise.resolve();
  expect(
    await callExtension(action, 'auth', () => {
      calls++;
      return Promise.resolve('unused');
    })
  ).toEqual({ status: 'stopped' });
  gate.resolve('late');
  await task;
  expect(calls).toBe(0);
});
