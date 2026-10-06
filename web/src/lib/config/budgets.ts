// Browser relay scopes are independent; these are application admission caps,
// not a DNS policy, native transport limit or browser memory guarantee.
export const RELAY_BUDGETS = Object.freeze({ public: 3, inbox: 3 });
