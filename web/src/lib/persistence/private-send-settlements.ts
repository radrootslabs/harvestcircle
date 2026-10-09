import {
  privateGiftWrapAttemptEvidence,
  type PrivateGiftWrapAttemptResult
} from '../nostr/private-pool.ts';
import { privateRecordSnapshot } from './private-records.ts';
import { loadPrivateRecord } from './private-storage.ts';
import {
  preparePrivateReceiptTransition,
  commitPrivateReceiptTransition,
  type PrivateReceiptCommitResult
} from './private-receipts.ts';
// Late bookkeeping is bound to the original actual EVENT, owner and immutable
// ciphertexts. It performs no SDK work, resumes nothing and renews no ACK.
export async function reconcilePrivateSendAttempt(
  result: PrivateGiftWrapAttemptResult
): Promise<PrivateReceiptCommitResult> {
  const original = privateGiftWrapAttemptEvidence(result);
  if (!original || typeof window === 'undefined' || !navigator.locks?.request)
    return { status: 'invalid' };
  try {
    return await navigator.locks.request(
      'harvestcircle:owner:' + original.owner,
      { mode: 'exclusive' },
      async () => {
        const loaded = await loadPrivateRecord(
          original.repository,
          'private_sends',
          original.command
        );
        if (!loaded.ok) return { status: loaded.reason };
        const row = privateRecordSnapshot(
          loaded.value,
          original.owner,
          original.command
        );
        if (
          !row ||
          row.family !== 'private_send_operation' ||
          JSON.stringify({
            owner: row.owner,
            id: row.id,
            peer: row.peer,
            rumorHash: row.rumorHash,
            createdAt: row.createdAt,
            self: row.self,
            peerArtifact: row.peerArtifact
          }) !== original.pairWire
        )
          return { status: 'conflict' };
        const transition = preparePrivateReceiptTransition(
          loaded.value,
          original.owner,
          original.command,
          original.receiptWire
        );
        if (!transition.ok)
          return {
            status: transition.reason === 'conflict' ? 'conflict' : 'invalid'
          };
        return commitPrivateReceiptTransition(
          original.repository,
          transition.value
        );
      }
    );
  } catch {
    return { status: 'unknown_completion' };
  }
}
