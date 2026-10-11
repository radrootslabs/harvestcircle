import { validateSupportPolicy } from './support.ts';

// Actual support owner/destination have not been supplied. These empty public
// inputs allow source development; the named launch preparation gate refuses
// them. Controlled fixture destinations never become production defaults.
const policy = validateSupportPolicy(
  JSON.stringify({
    schemaVersion: 1,
    owner: '',
    destination: ''
  })
);
if (!policy) throw Error('support_policy_invalid');
export const deploymentSupportPolicy = policy;
