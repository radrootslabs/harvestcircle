import {
  capturePrivatePublication,
  type PrivateDeliveryRole
} from '../nostr/private-publisher.ts';
import {
  publishPrivateGiftWrapAttempt,
  type PrivatePool,
  type PrivateGiftWrapAttemptResult
} from '../nostr/private-pool.ts';
import type { PrivateStorageRepository } from '../persistence/private-storage.ts';
import type { PairedDeliveryAcknowledgement } from '../persistence/private-sends.ts';
import type { PrivateSession } from '../runtime/private-session.ts';
import type { PairedDeliveryContext } from './prepare-send.ts';
// One explicitly reviewed persisted role/target effect only. Named durable
// receipts, bounded stored retries and approved routing changes have later
// owners. An accepted relay attempt is never a person-delivered result.
export async function deliverPrivateSend(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  receipt: PairedDeliveryAcknowledgement,
  context: PairedDeliveryContext,
  pool: PrivatePool,
  role: PrivateDeliveryRole,
  origin: string,
  review: unknown,
  signal: AbortSignal
): Promise<PrivateGiftWrapAttemptResult | Readonly<{ status: 'invalid' }>> {
  const permission = capturePrivatePublication(
    repository,
    session,
    receipt,
    context,
    role,
    origin,
    review
  );
  return permission
    ? publishPrivateGiftWrapAttempt(pool, permission, signal)
    : { status: 'invalid' };
}
