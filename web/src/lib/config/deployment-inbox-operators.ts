import { deploymentRelayPolicy } from './deployment-relays.ts';
import { readRelayPolicy } from './relays.ts';
import { validateInboxOperatorQualification } from './inbox-operators.ts';

// No operator capability evidence has been supplied for production. Keep this
// source-owned input unconfigured; controlled relay fixtures do not qualify it.
const qualification = validateInboxOperatorQualification(
  JSON.stringify({
    schemaVersion: 1,
    revision: 1,
    authority: 'unconfigured',
    policyWire: JSON.stringify(readRelayPolicy(deploymentRelayPolicy)),
    evidenceSha256: null,
    relays: []
  })
);
if (!qualification) throw new Error('inbox_operator_configuration_invalid');

export const deploymentInboxOperatorQualification = qualification;
