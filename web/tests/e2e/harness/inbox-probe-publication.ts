import { finalize } from 'rxjs';
export { makeFixture as makeOrdinaryPublicationFixture } from './private-publisher.ts';
import { makeProbeFixture } from './inbox-probe.ts';
import { RelayPool } from 'applesauce-relay/pool';
import {
  getPrivatePool,
  publishInboxProbeArchiveAttempt,
  closePrivatePool,
  type PrivatePool,
  type InboxProbeArchiveAttemptResult
} from '../../../src/lib/nostr/private-pool.ts';
import {
  captureInboxProbePublicationAction,
  captureInboxProbePublication,
  type InboxProbePublicationAction
} from '../../../src/lib/nostr/private-publisher.ts';
import {
  inboxProbeAcknowledgementSnapshot,
  commitInboxProbeArchiveAttempt
} from '../../../src/lib/persistence/inbox-probes.ts';
export * from './inbox-probe.ts';
export * from '../../../src/lib/nostr/private-publisher.ts';
export * from '../../../src/lib/nostr/private-pool.ts';
export async function makePublicationFixture() {
  const f = await makeProbeFixture();
  let pool: PrivatePool | undefined,
    last: InboxProbeArchiveAttemptResult | undefined;
  let action: InboxProbePublicationAction | undefined;
  const consumer = Object.freeze({});
  function capture() {
    const acknowledgement = f.acknowledgement();
    if (!acknowledgement) return undefined;
    const next = captureInboxProbePublicationAction(
      f.repository,
      f.session,
      f.policy,
      acknowledgement,
      'reviewed_inbox_probe_network'
    );
    action ??= next;
    return next;
  }
  function permission(
    origin = 'wss://inbox.example.org',
    review: unknown = 'reviewed_inbox_probe_publication'
  ) {
    const acknowledgement = f.acknowledgement(),
      original = capture();
    return acknowledgement && original
      ? captureInboxProbePublication(
          f.repository,
          f.session,
          f.policy,
          acknowledgement,
          original,
          origin,
          review
        )
      : undefined;
  }
  function acquire() {
    pool ??= getPrivatePool(
      f.session,
      f.policy,
      f.acknowledgement()
        ? inboxProbeAcknowledgementSnapshot(f.acknowledgement()!)!.record
            .binding.archiveTargets
        : ['wss://inbox.example.org'],
      consumer,
      'delivery'
    );
    if (!pool) throw Error('actual source delivery scope unavailable');
    return pool;
  }
  return {
    ...f,
    action: capture,
    permission,
    pool: acquire,
    snapshot: () =>
      f.acknowledgement() &&
      inboxProbeAcknowledgementSnapshot(f.acknowledgement()!),
    async publish(token = permission(), signal = new AbortController().signal) {
      if (!token) return { status: 'stopped' as const };
      last = await publishInboxProbeArchiveAttempt(acquire(), token, signal);
      return last;
    },
    last: () => last,
    append: (result = last) =>
      result
        ? commitInboxProbeArchiveAttempt(f.repository, result)
        : Promise.resolve({ status: 'invalid' as const }),
    installNetworkClock() {
      const original = performance.now.bind(performance);
      let now = original(),
        observations = 0;
      // Only the actual shared publication meter samples this controlled clock.
      // Public discovery/head/lease lifetimes and Date.now remain real.
      performance.now = () => {
        if (/at clock(?:\d+)? \(/.test(new Error().stack ?? '')) {
          observations++;
          return now;
        }
        return original();
      };
      return {
        set: (value: number) => {
          now = value;
        },
        advance: (value: number) => {
          now += value;
        },
        observations: () => observations,
        restore: () => {
          performance.now = original;
        }
      };
    },
    installAttemptTeardownFailure(onTeardown?: () => void) {
      const original = Object.getOwnPropertyDescriptor(
        RelayPool.prototype,
        'relay'
      )?.value as RelayPool['relay'];
      const restorations: (() => void)[] = [];
      let failing = true,
        faults = 0;
      RelayPool.prototype.relay = function (
        ...args: Parameters<RelayPool['relay']>
      ) {
        const relay = original.apply(this, args),
          event = relay.event.bind(relay);
        relay.event = (...eventArgs: Parameters<typeof relay.event>) =>
          event(...eventArgs).pipe(
            finalize(() => {
              if (failing) {
                onTeardown?.();
                faults++;
                throw Error(
                  'controlled original SDK observable teardown failure'
                );
              }
            })
          );
        restorations.push(() => {
          relay.event = event;
        });
        return relay;
      };
      return {
        faults: () => faults,
        restore: () => {
          failing = false;
          RelayPool.prototype.relay = original;
          for (const restoration of restorations) restoration();
        }
      };
    },
    installAcquisitionFailure() {
      const original = Object.getOwnPropertyDescriptor(
        RelayPool.prototype,
        'relay'
      )?.value as RelayPool['relay'];
      RelayPool.prototype.relay = () => {
        throw Error('controlled actual SDK metadata acquisition failure');
      };
      return () => {
        RelayPool.prototype.relay = original;
      };
    },
    stop: () => {
      if (pool) closePrivatePool(pool);
    },
    close() {
      if (pool) closePrivatePool(pool);
      f.close();
    }
  };
}
