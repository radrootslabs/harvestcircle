import { deploymentRelayPolicy } from '../src/lib/config/deployment-relays.ts';
import { requireLaunchRelayPolicy } from '../src/lib/config/relays.ts';

try {
  requireLaunchRelayPolicy(deploymentRelayPolicy);
  console.log(
    'Relay launch inputs admitted; operator qualification remains required'
  );
} catch {
  console.error('relay_launch_inputs_missing');
  process.exitCode = 1;
}
