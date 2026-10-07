import { PUBLIC_PUBLICATION_BUDGETS } from '../config/budgets.ts';
import type { RelayPolicy } from '../config/relays.ts';
import {
  publishPublicPreferenceAttempt,
  type PublicPool
} from './public-pool.ts';
import type { PreferencePublication } from './inbox-preference-publication.ts';
import { canonicalLocalId } from '../private-handles.ts';
import {
  loadPreferenceOperation,
  commitPublicOperationTransition,
  type PublicQuotaRepository
} from '../persistence/quota.ts';
import { preparePublicReceiptTransition } from '../persistence/artifact-records.ts';
import { publicRecordSnapshot } from '../persistence/records.ts';
export type PreferencePublicationResult = Readonly<{
  status:
    'completed' | 'stopped' | 'storage_failed' | 'invalid' | 'needs_action';
}>;
// One explicitly resumed finite action. Caller owns the real current identity,
// known-base lookup and opaque authorization, never arbitrary event templates.
export async function publishInboxPreference(
  repository: PublicQuotaRepository,
  owner: string,
  id: string,
  actionId: string,
  pool: PublicPool,
  policy: RelayPolicy,
  signal: AbortSignal,
  authorize: () => Promise<PreferencePublication | undefined>
): Promise<PreferencePublicationResult> {
  if (!canonicalLocalId(actionId)) return { status: 'invalid' };
  const started = performance.now();
  const initial = await loadPreferenceOperation(repository, id);
  if (!initial.ok) return { status: 'storage_failed' };
  const row = publicRecordSnapshot(initial.value, owner, id);
  if (
    row?.family !== 'preference_operation' ||
    !row.artifact ||
    row.revision < 2
  )
    return { status: 'invalid' };
  for (const origin of row.capture.targets) {
    if (
      row.receipts.some(
        (fact) =>
          fact.origin === origin &&
          fact.eventId === row.artifact!.eventId &&
          fact.status === 'accepted'
      )
    )
      continue;
    for (
      let attempt = 1;
      attempt <= PUBLIC_PUBLICATION_BUDGETS.attemptsPerTargetAction;
      attempt++
    ) {
      if (
        signal.aborted ||
        performance.now() - started >=
          PUBLIC_PUBLICATION_BUDGETS.networkActionMilliseconds
      )
        return { status: 'stopped' };
      const permission = await authorize();
      const remaining =
        PUBLIC_PUBLICATION_BUDGETS.networkActionMilliseconds -
        (performance.now() - started);
      if (!permission || signal.aborted || remaining <= 0)
        return { status: 'stopped' };
      const result = await publishPublicPreferenceAttempt(
        pool,
        permission,
        policy,
        origin,
        signal,
        remaining
      );
      const read = await loadPreferenceOperation(repository, id);
      if (!read.ok) return { status: 'storage_failed' };
      const transition = preparePublicReceiptTransition(
        read.value,
        owner,
        id,
        JSON.stringify({
          actionId,
          origin,
          role: 'preference',
          attempt,
          eventId: row.artifact.eventId,
          status: result.status,
          observedAtMilliseconds: Date.now(),
          readbackWire: null
        })
      );
      if (
        !transition.ok ||
        !(await commitPublicOperationTransition(repository, transition.value))
          .ok
      )
        return { status: 'storage_failed' };
      if (result.status === 'accepted') break;
      if (result.status !== 'timed_out')
        return {
          status: result.status === 'stopped' ? 'stopped' : 'needs_action'
        };
      if (attempt === PUBLIC_PUBLICATION_BUDGETS.attemptsPerTargetAction)
        return { status: 'needs_action' };
    }
  }
  return { status: 'completed' };
}
