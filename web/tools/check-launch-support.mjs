import { deploymentSupportPolicy } from '../src/lib/config/deployment-support.ts';
import { requireLaunchSupportPolicy } from '../src/lib/config/support.ts';

try {
  requireLaunchSupportPolicy(deploymentSupportPolicy);
  console.log(
    'Support launch inputs admitted; actual operator qualification remains required'
  );
} catch {
  console.error('support_launch_inputs_missing');
  process.exitCode = 1;
}
