// Browser relay scopes are independent; these are application admission caps,
// not a DNS policy, native transport limit or browser memory guarantee.
const publicRelayLimit = 3;
export const RELAY_BUDGETS = Object.freeze({
  public: publicRelayLimit,
  inbox: 3
});

// Finite private transport page; duplicates and rejected ciphertext count.
// Full history/live coordination has separate owners. These are logical
// post-SDK-parse limits, not browser WebSocket allocation guarantees.
export const PRIVATE_TRANSPORT_BUDGETS = Object.freeze({
  bodyBytes: 4096,
  rumorBytes: 8192,
  sealBytes: 16384,
  requestedPerRelay: 200,
  deliveries: 500,
  processedBytes: 8388608,
  pageMilliseconds: 15000,
  envelopeBytes: 32768
});

// Original M06/M07 authority: responses are counted before permission work,
// per actual connection and explicit action, never renewed by challenge churn.
export const PRIVATE_AUTH_BUDGETS = Object.freeze({
  responsesPerConnectionAction: 2
});

// The private explicit-action network budget begins after preparation and
// provider permission waiting. Later retry scheduling shares this same cap.
export const PRIVATE_PUBLICATION_BUDGETS = Object.freeze({
  attemptsPerTargetAction: 3,
  networkActionMilliseconds: 45000
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
  pageRows: 20,
  publisherAuthorsPerPage: 20
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

// Browser-local logical admission, independent of desktop SQLite and the public
// memory store. Repository transactions enforce aggregate counts/bytes later.
export const LOCAL_PERSISTENCE_BUDGETS = Object.freeze({
  publicDrafts: 20,
  draftComposedBytes: 16384,
  publicOperations: 100,
  publicOperationBytes: 8388608,
  unfinishedPrivateSends: 100,
  privateSendBytes: 8388608,
  receivedEnvelopes: 2000,
  receivedCiphertextBytes: 50331648
});

// Approved explicit public publication action; user approval time is separate
// from this future network scheduler bound. Codecs validate attempt metadata.
export const PUBLIC_PUBLICATION_BUDGETS = Object.freeze({
  attemptsPerTargetAction: 3,
  networkActionMilliseconds: 45000
});
