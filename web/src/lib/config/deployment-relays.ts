import { validateRelayPolicy } from './relays.ts';

// No production endpoints/operators have been approved. Keep source development
// possible without guessing egress; the explicit launch-input command rejects
// these empty sets. Operation consumers must enforce the two separate flags.
const policy = validateRelayPolicy(
  JSON.stringify({
    schemaVersion: 1,
    public: [],
    inbox: [],
    postingEnabled: false,
    messagingEnabled: false,
    operatorDenylist: []
  })
);
if (!policy) throw new Error('relay_policy_invalid');

export const deploymentRelayPolicy = policy;
