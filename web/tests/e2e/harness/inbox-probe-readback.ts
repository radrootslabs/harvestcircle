import { finalize, tap, map } from 'rxjs';
import { RelayPool } from 'applesauce-relay/pool';
import { makePublicationFixture } from './inbox-probe-publication.ts';
import { captureInboxProbeReadback } from '../../../src/lib/nostr/private-publisher.ts';
import {
  getPrivatePool,
  readInboxProbeAttempt,
  closePrivatePool,
  type PrivatePool,
  type InboxProbeReadbackAttemptResult
} from '../../../src/lib/nostr/private-pool.ts';
import { commitInboxProbeReadbackAttempt } from '../../../src/lib/persistence/inbox-probes.ts';
export * from './inbox-probe-publication.ts';
export async function makeReadbackFixture() {
  const f = await makePublicationFixture();
  let pool: PrivatePool | undefined,
    last: InboxProbeReadbackAttemptResult | undefined;
  const consumer = Object.freeze({});
  function permission(
    origin = 'wss://inbox.example.org',
    review: unknown = 'reviewed_inbox_probe_readback'
  ) {
    const ack = f.acknowledgement(),
      action = f.action();
    return ack && action
      ? captureInboxProbeReadback(
          f.repository,
          f.session,
          f.policy,
          ack,
          action,
          origin,
          review
        )
      : undefined;
  }
  function acquire() {
    pool ??= getPrivatePool(
      f.session,
      f.policy,
      f.snapshot()!.record.binding.readTargets,
      consumer,
      'access'
    );
    if (!pool) throw Error('actual original access scope');
    return pool;
  }
  return {
    ...f,
    readPermission: permission,
    readPool: acquire,
    async read(token = permission(), signal = new AbortController().signal) {
      if (!token) return { status: 'stopped' as const };
      last = await readInboxProbeAttempt(acquire(), token, signal);
      return last;
    },
    lastRead: () => last,
    appendRead: (result = last) =>
      result
        ? commitInboxProbeReadbackAttempt(f.repository, result)
        : Promise.resolve({ status: 'invalid' as const }),
    installWrongReadSource() {
      const original = Object.getOwnPropertyDescriptor(
        RelayPool.prototype,
        'req'
      )?.value as RelayPool['req'];
      RelayPool.prototype.req = function (
        ...args: Parameters<RelayPool['req']>
      ) {
        return original
          .apply(this, args)
          .pipe(
            map((message) =>
              message.type === 'EVENT'
                ? { ...message, from: 'wss://foreign.example.org' }
                : message
            )
          );
      };
      return () => {
        RelayPool.prototype.req = original;
      };
    },
    installReadInstallation(onInstallation: () => void) {
      const original = Object.getOwnPropertyDescriptor(
        RelayPool.prototype,
        'req'
      )?.value as RelayPool['req'];
      let fired = false;
      RelayPool.prototype.req = function (
        ...args: Parameters<RelayPool['req']>
      ) {
        return original.apply(this, args).pipe(
          tap((message) => {
            if (!fired && message.type === 'OPEN') {
              fired = true;
              onInstallation();
            }
          })
        );
      };
      return () => {
        RelayPool.prototype.req = original;
      };
    },
    installReadTeardownFailure(onTeardown?: () => void) {
      const original = Object.getOwnPropertyDescriptor(
        RelayPool.prototype,
        'req'
      )?.value as RelayPool['req'];
      let failing = true,
        faults = 0;
      RelayPool.prototype.req = function (
        ...args: Parameters<RelayPool['req']>
      ) {
        return original.apply(this, args).pipe(
          finalize(() => {
            if (failing) {
              faults++;
              onTeardown?.();
              throw Error('actual original REQ teardown failure');
            }
          })
        );
      };
      return {
        faults: () => faults,
        restore: () => {
          failing = false;
          RelayPool.prototype.req = original;
        }
      };
    },
    stopRead: () => {
      if (pool) closePrivatePool(pool);
    },
    close() {
      if (pool) closePrivatePool(pool);
      f.close();
    }
  };
}
