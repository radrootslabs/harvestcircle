import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../../../src/lib/nostr/verified-envelope.ts';
import {
  PRIVATE_AUTH_BUDGETS,
  PRIVATE_TRANSPORT_BUDGETS
} from '../../../src/lib/config/budgets.ts';
import type { NostrEvent } from 'applesauce-core/helpers';

export const fixtureRelay = 'wss://inbox.example.org/';
// Expected-policy loopback model, never a deployment/operator/retention claim.
// Clock and declared retention are explicit test inputs. Genuine SDK signatures
// are verified afresh across JSON; the relay cannot inspect encrypted senders.
export function createInboxPolicy(options: {
  now: number;
  retentionSeconds: number;
  requireOuterAuthor?: boolean;
}) {
  if (
    !Number.isSafeInteger(options.now) ||
    options.now < 0 ||
    !Number.isSafeInteger(options.retentionSeconds) ||
    options.retentionSeconds < 48 * 3600 + 300
  )
    throw new Error('invalid fixture policy');
  const events = new Map<string, NostrEvent>();
  let bytes = 0;
  function event(value: unknown): NostrEvent | undefined {
    const wire = JSON.stringify(value);
    if (
      typeof wire !== 'string' ||
      new TextEncoder().encode(wire).length >
        PRIVATE_TRANSPORT_BUDGETS.envelopeBytes
    )
      return undefined;
    const verified = verifyEnvelope(wire);
    return verified.ok ? verifiedEnvelopeSnapshot(verified.value) : undefined;
  }
  return {
    size: () => events.size,
    connect(challenge: string) {
      let owner: string | undefined,
        closed = false,
        responses = 0;
      return {
        close() {
          closed = true;
          owner = undefined;
        },
        frame(raw: string): unknown[][] {
          if (closed) return [['NOTICE', 'closed: fixture connection']];
          let frame: unknown;
          try {
            if (new TextEncoder().encode(raw).length > 65536)
              throw new Error('fixture frame too large');
            frame = JSON.parse(raw);
          } catch {
            return [['NOTICE', 'invalid: fixture frame']];
          }
          if (!Array.isArray(frame))
            return [['NOTICE', 'invalid: fixture frame']];
          if (frame[0] === 'AUTH' || frame[0] === 'EVENT') {
            const e = event(frame[1]),
              id = e?.id ?? '';
            if (frame.length !== 2)
              return [['OK', id, false, 'invalid: fixture envelope']];
            if (frame[0] === 'AUTH') {
              owner = undefined;
              responses++;
              if (
                !e ||
                responses > PRIVATE_AUTH_BUDGETS.responsesPerConnectionAction ||
                e.kind !== 22242 ||
                e.content !== '' ||
                e.tags.length !== 2 ||
                !e.tags.some(
                  (t) =>
                    t.length === 2 && t[0] === 'relay' && t[1] === fixtureRelay
                ) ||
                !e.tags.some(
                  (t) =>
                    t.length === 2 && t[0] === 'challenge' && t[1] === challenge
                )
              )
                return [['OK', id, false, 'restricted: invalid AUTH']];
              owner = e.pubkey;
              return [['OK', id, true, '']];
            }
            if (!owner)
              return [['OK', id, false, 'auth-required: fixture inbox']];
            if (
              !e ||
              e.kind !== 1059 ||
              e.content.length === 0 ||
              e.tags.length !== 1 ||
              e.tags[0].length !== 2 ||
              e.tags[0][0] !== 'p' ||
              !/^[0-9a-f]{64}$/u.test(e.tags[0][1]) ||
              e.created_at < options.now - options.retentionSeconds ||
              e.created_at > options.now + 300 ||
              (options.requireOuterAuthor && e.pubkey !== owner)
            )
              return [
                ['OK', id, false, 'restricted: unsupported outer envelope']
              ];
            if (events.has(e.id)) return [['OK', id, true, '']];
            const size = new TextEncoder().encode(JSON.stringify(e)).length;
            if (
              events.size >= PRIVATE_TRANSPORT_BUDGETS.deliveries ||
              bytes + size > PRIVATE_TRANSPORT_BUDGETS.processedBytes
            )
              return [['OK', id, false, 'restricted: fixture capacity']];
            events.set(e.id, e);
            bytes += size;
            return [['OK', id, true, '']];
          }
          if (frame[0] === 'CLOSE') return [];
          if (
            frame[0] !== 'REQ' ||
            typeof frame[1] !== 'string' ||
            frame[1].length > 64
          )
            return [['NOTICE', 'invalid: fixture request']];
          const id = frame[1],
            query: unknown = frame[2];
          if (!owner) return [['CLOSED', id, 'auth-required: fixture inbox']];
          if (
            frame.length !== 3 ||
            !query ||
            typeof query !== 'object' ||
            Array.isArray(query)
          )
            return [['CLOSED', id, 'restricted: recipient-only']];
          const q = query as Record<string, unknown>;
          if (
            Object.keys(q).length !== 3 ||
            !Array.isArray(q.kinds) ||
            q.kinds.length !== 1 ||
            q.kinds[0] !== 1059 ||
            !Array.isArray(q['#p']) ||
            q['#p'].length !== 1 ||
            q['#p'][0] !== owner ||
            typeof q.limit !== 'number' ||
            !Number.isSafeInteger(q.limit) ||
            q.limit < 1 ||
            q.limit > PRIVATE_TRANSPORT_BUDGETS.requestedPerRelay
          )
            return [['CLOSED', id, 'restricted: recipient-only']];
          const rows = [...events.values()]
            .filter((e) => e.tags[0][1] === owner)
            .sort(
              (a, b) => b.created_at - a.created_at || a.id.localeCompare(b.id)
            )
            .slice(0, q.limit);
          return [
            ...rows.map((e) => [
              'EVENT',
              id,
              JSON.parse(JSON.stringify(e)) as unknown
            ]),
            ['EOSE', id]
          ];
        }
      };
    }
  };
}
