import { operationEnabled } from '../config/operation-flags.ts';
import type { RelayPolicy } from '../config/relays.ts';

// Detached view information. Original effect owners still validate every
// owner/template/target/native receipt and explicit action independently.
export function operationCapabilities(policy?: RelayPolicy) {
  return {
    posting: operationEnabled(policy, 'posting'),
    messaging: operationEnabled(policy, 'messaging'),
    newListing: operationEnabled(policy, 'new_listing'),
    draft: true as const,
    pendingReview: true as const,
    qualification: 'not_established_by_flags' as const
  };
}
