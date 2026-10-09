import {
  privateRecordSnapshot,
  type PrivateRecordHandle,
  type PrivateTargetReceipt
} from '../persistence/private-records.ts';
// Pure detached status from strict local evidence, never EventStore seen-relay
// metadata, a remote read receipt, effect permission or a person-delivery claim.
export function privateSendStatus(
  record: PrivateRecordHandle,
  owner: unknown,
  id: unknown
) {
  const row = privateRecordSnapshot(record, owner, id);
  if (!row || row.family !== 'private_send_operation') return undefined;
  const facts = row.receipts ?? [];
  function role(role: 'peer' | 'self_archive', targets: readonly string[]) {
    const matching = facts.filter((f) => f.role === role);
    function origins(status: PrivateTargetReceipt['status']) {
      return targets.filter((origin) =>
        matching.some((f) => f.origin === origin && f.status === status)
      );
    }
    const acceptedTargets = origins('accepted'),
      readbackTargets = origins('readback');
    return {
      acceptedTargets,
      readbackTargets,
      pendingTargets: targets.filter(
        (origin) => !acceptedTargets.includes(origin)
      ),
      refusedTargets: origins('refused'),
      timeoutTargets: origins('timed_out'),
      unknownTargets: origins('unknown'),
      stoppedTargets: origins('stopped')
    };
  }
  const recipientFacts = role(
      'peer',
      row.deliveryPlan?.routes.peer.targets ?? []
    ),
    archiveFacts = role(
      'self_archive',
      row.deliveryPlan?.routes.archive.targets ?? []
    );
  const recipient = {
      ...recipientFacts,
      state: recipientFacts.acceptedTargets.length
        ? ('accepted_by_inbox_relay' as const)
        : ('unknown' as const)
    },
    archive = {
      ...archiveFacts,
      state:
        archiveFacts.acceptedTargets.length &&
        !archiveFacts.pendingTargets.length
          ? ('accepted_by_inbox_relay' as const)
          : ('pending' as const)
    };
  const anyAccepted =
      recipient.acceptedTargets.length + archive.acceptedTargets.length > 0,
    partial =
      anyAccepted &&
      (recipient.state === 'unknown' ||
        archive.state === 'pending' ||
        recipient.pendingTargets.length > 0);
  const labels: string[] = [
    'Saved encrypted locally',
    recipient.state === 'accepted_by_inbox_relay'
      ? 'Accepted by recipient inbox relay'
      : 'Recipient delivery unknown',
    ...(partial ? ['Partial delivery'] : []),
    ...(archive.state === 'pending' ? ['Sender archive pending'] : [])
  ];
  return { recipient, archive, partial, labels };
}
