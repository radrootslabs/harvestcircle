import { RelayPool } from 'applesauce-relay/pool';
// One implementation, distinct SDK instances and relay/AUTH stores. Only owned
// adapters call this factory; no SDK/options are exposed through UI capabilities.
export function createScopedRelayPool(): RelayPool {
  if (typeof window === 'undefined') throw new Error('relay_pool_browser_only');
  return new RelayPool({
    keepAlive: 0,
    enablePing: false,
    requestReconnect: 0,
    subscriptionReconnect: 0
  });
}
