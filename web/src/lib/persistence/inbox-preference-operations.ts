import {
  captureInboxSetup,
  type InboxSetupReview
} from '../messaging/inbox-setup.ts';
import type { InboxResolver } from '../messaging/resolve-inbox.ts';
import type { PublicRecordHandle } from './records.ts';
import {
  claimPublicOperation,
  publicQuotaOwner,
  type PublicQuotaRepository,
  type PublicQuotaFailure
} from './quota.ts';
export type InboxPreferenceSave =
  | Readonly<{ status: 'saved' | 'existing'; record: PublicRecordHandle }>
  | Readonly<{
      status:
        'invalid' | 'unavailable' | 'conflict' | 'busy' | PublicQuotaFailure;
    }>;
// Preparation only; exact preference store/CAS/quota remains in its existing
// owner. No extension or network await occurs inside the short transaction.
export async function saveInboxPreferenceOperation(
  repository: PublicQuotaRepository,
  review: InboxSetupReview,
  resolver: InboxResolver,
  command: unknown,
  consent: unknown
): Promise<InboxPreferenceSave> {
  try {
    const initial = captureInboxSetup(review, resolver, command, consent);
    if (initial.status !== 'captured') return initial;
    if (publicQuotaOwner(repository) !== initial.owner)
      return { status: 'invalid_scope' };
    if (typeof window === 'undefined' || !navigator.locks?.request)
      return { status: 'unavailable' };
    return await navigator.locks.request(
      'harvestcircle:owner:' + initial.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock): Promise<InboxPreferenceSave> => {
        if (!lock) return { status: 'busy' };
        const fresh = captureInboxSetup(review, resolver, command, consent);
        if (fresh.status !== 'captured') return fresh;
        if (publicQuotaOwner(repository) !== fresh.owner)
          return { status: 'invalid_scope' };
        const claimed = await claimPublicOperation(
          repository,
          command,
          fresh.record
        );
        if (!claimed.ok) return { status: claimed.reason };
        return {
          status: claimed.value.state === 'created' ? 'saved' : 'existing',
          record: claimed.value.record
        };
      }
    );
  } catch {
    return { status: 'unavailable' };
  }
}
