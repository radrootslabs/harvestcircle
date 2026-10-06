// Browser relay scopes are independent; these are application admission caps,
// not a DNS policy, native transport limit or browser memory guarantee.
const publicRelayLimit = 3;
export const RELAY_BUDGETS = Object.freeze({
  public: publicRelayLimit,
  inbox: 3
});

// Approved public-run admission limits shared by all primary/auxiliary sources.
// Logical post-parse work accounting; no WebSocket preallocation guarantee.
const publicDeliveryLimit = 2000;
export const PUBLIC_INGRESS_BUDGETS = Object.freeze({
  deliveries: publicDeliveryLimit,
  processedBytes: 8388608,
  eventBytes: 262144
});

// Shared by primary discovery and every auxiliary head/deletion/profile scope.
export const PUBLIC_REQUEST_BUDGETS = Object.freeze({
  parallelScopes: 6,
  runMilliseconds: 15000,
  requestMilliseconds: 10000
});

// Exact-coordinate auxiliary reads use the approved chronological page size.
// Query input admission is bounded by the existing run delivery ceiling;
// coherent logical working-set retention is a separate policy.
export const PUBLIC_QUERY_BUDGETS = Object.freeze({
  requestedPerRelay: 200,
  coordinatesPerRun: publicDeliveryLimit
});

// Shared logical retained public-event payload across every view and run.
export const PUBLIC_RETENTION_BUDGETS = Object.freeze({
  payloadBytes: 33554432
});

// Query text is public input. Bound both original and normalized UTF-8 copies.
export const PUBLIC_SEARCH_BUDGETS = Object.freeze({
  queryBytes: 512,
  terms: 12,
  pageRows: 20
});

// One NIP-50 round may select each of the three qualified sources separately;
// chronological discovery has two group windows. Count failed attempts too.
const publicChronologicalWindows = 2;
export const PUBLIC_SEARCH_RUN_BUDGETS = Object.freeze({
  nip50Rounds: 1,
  chronologicalWindows: publicChronologicalWindows,
  primaryScopes: publicRelayLimit + publicChronologicalWindows
});

// Optional relevance sample: requested count and actual candidate work per source.
// The shared run meter still charges duplicates, rejects and every auxiliary.
export const PUBLIC_NIP50_BUDGETS = Object.freeze({
  requestedPerRelay: 100,
  candidatesPerSource: 100
});
