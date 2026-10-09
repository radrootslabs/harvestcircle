import {
  PRIVATE_TRANSPORT_BUDGETS,
  PRIVATE_LIVE_BUDGETS,
  LOCAL_PERSISTENCE_BUDGETS
} from '../config/budgets.ts';
// Detached display/recovery metadata only. Original transport/repositories own
// actual admission; these choices never reset a meter, delete or resume work.
export function inboxBudgetSnapshot() {
  return {
    finite: {
      deliveries: PRIVATE_TRANSPORT_BUDGETS.deliveries,
      bytes: PRIVATE_TRANSPORT_BUDGETS.processedBytes,
      milliseconds: PRIVATE_TRANSPORT_BUDGETS.pageMilliseconds
    },
    live: {
      deliveries: PRIVATE_LIVE_BUDGETS.deliveries,
      bytes: PRIVATE_LIVE_BUDGETS.processedBytes,
      milliseconds: PRIVATE_LIVE_BUDGETS.windowMilliseconds
    },
    received: {
      records: LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes,
      bytes: LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes
    },
    outbox: {
      records: LOCAL_PERSISTENCE_BUDGETS.unfinishedPrivateSends,
      bytes: LOCAL_PERSISTENCE_BUDGETS.privateSendBytes
    }
  };
}
export function inboxRecoveryChoices(reason: unknown): readonly string[] {
  if (typeof reason !== 'string') return [];
  if (reason === 'capacity')
    return ['review_local_received_copies', 'review_relay_recovery'];
  if (reason === 'budget' || reason === 'timeout' || reason === 'elapsed')
    return [
      'wait_then_review_refresh',
      'review_history_scope',
      'review_local_received_copies'
    ];
  if (
    [
      'aborted',
      'unknown_completion',
      'conflict',
      'corrupt_record',
      'unavailable'
    ].includes(reason)
  )
    return ['review_storage_recovery'];
  return [];
}
