import { readRelayPolicy, type RelayPolicy } from '../config/relays.ts';

// Pure destination qualification avoids importing query normalization or its
// query-copy encoder into anonymous transport activation.
export function qualifiedNip50Sources(policy: RelayPolicy): readonly string[] {
  return readRelayPolicy(policy)
    .public.filter((source) => source.read && source.nip50)
    .map((source) => source.origin);
}
export function requireNip50Source(
  policy: RelayPolicy,
  source: string
): string {
  if (!qualifiedNip50Sources(policy).includes(source))
    throw new Error('nip50_source_unqualified');
  return source;
}
