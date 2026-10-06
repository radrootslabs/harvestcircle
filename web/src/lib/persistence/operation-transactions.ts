// Pure storage mechanics shared by typed operation repositories. This module
// acquires no database, signer, relay, raw-event publisher or account authority.
// Matching the original base is an observation, not proof of global absence.
export function decideFrozenTransition(
  currentWire: unknown,
  baseWire: string,
  nextWire: string
): 'committed' | 'base_observed' | 'conflict' {
  if (currentWire === nextWire) return 'committed';
  return currentWire === baseWire ? 'base_observed' : 'conflict';
}
