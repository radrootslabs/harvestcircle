import {
  inboxPreferenceSnapshot,
  type InboxPreference
} from '../nostr/inbox-preferences.ts';

// Detached public preference presentation only. No missing/ready conclusion,
// guessed destination, connection, provider, persistence or setup effect.
export function inboxProfileSnapshot(preference: InboxPreference):
  | Readonly<{
      author: string;
      id: string;
      createdAt: number;
      status: 'compatible' | 'unsupported';
      advertised: string[];
    }>
  | undefined {
  const saved = inboxPreferenceSnapshot(preference);
  return (
    saved && {
      author: saved.author,
      id: saved.id,
      createdAt: saved.createdAt,
      status: saved.status === 'supported' ? 'compatible' : 'unsupported',
      advertised: saved.relays
    }
  );
}
