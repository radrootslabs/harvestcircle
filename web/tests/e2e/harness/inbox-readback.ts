export * from './inbox-preference-publisher.ts';
export {
  createPublicRuntimeContext,
  mountPublicRuntime,
  createPublicView,
  beginPublicViewRun,
  disposePublicView,
  closePublicRuntime
} from '../../../src/lib/runtime/public-runtime.ts';
export {
  createPrivateSession,
  closePrivateSession
} from '../../../src/lib/runtime/private-session.ts';
export {
  resolveInboxReadback,
  inboxReadbackSnapshot,
  inboxReadbackEvidence,
  closeInboxReadback
} from '../../../src/lib/nostr/inbox-readback.ts';
export {
  verifyInboxSetupOperation,
  existingInboxSetupSnapshot
} from '../../../src/lib/messaging/inbox-setup-verification.ts';

import type { RequestClock } from '../../../src/lib/nostr/request-scope.ts';
export function makeReadbackClock() {
  const state: { next?: () => void } = {};
  const clock: RequestClock = {
    now: () => {
      const next = state.next;
      state.next = undefined;
      next?.();
      return performance.now();
    },
    schedule: (callback, delay) => {
      const timer = setTimeout(callback, delay);
      return () => clearTimeout(timer);
    }
  };
  return {
    clock,
    once: (callback: () => void) => {
      state.next = callback;
    }
  };
}

import { makePublisherFixture } from './inbox-preference-publisher.ts';
import {
  validateRelayPolicy,
  readRelayPolicy
} from '../../../src/lib/config/relays.ts';
export async function makeDisjointReadbackFixture() {
  const fixture = await makePublisherFixture();
  const manifest = readRelayPolicy(fixture.policy);
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...manifest,
      public: [
        {
          origin: 'wss://discovery.example.org',
          read: true,
          write: false,
          nip50: false
        },
        {
          origin: 'wss://publisher.example.org',
          read: false,
          write: true,
          nip50: false
        }
      ]
    })
  );
  if (!policy) throw new Error('invalid actual disjoint policy');
  return { ...fixture, policy };
}
