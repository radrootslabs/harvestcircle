// Browser relay scopes are independent; these are application admission caps,
// not a DNS policy, native transport limit or browser memory guarantee.
export const RELAY_BUDGETS = Object.freeze({ public: 3, inbox: 3 });

// Approved public-run admission limits shared by all primary/auxiliary sources.
// Logical post-parse work accounting; no WebSocket preallocation guarantee.
export const PUBLIC_INGRESS_BUDGETS = Object.freeze({
  deliveries: 2000,
  processedBytes: 8388608,
  eventBytes: 262144
});

// Shared by primary discovery and every auxiliary head/deletion/profile scope.
export const PUBLIC_REQUEST_BUDGETS = Object.freeze({
  parallelScopes: 6,
  runMilliseconds: 15000,
  requestMilliseconds: 10000
});
