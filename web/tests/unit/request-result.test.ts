import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { EventStore } from 'applesauce-core';
import { getSeenRelays } from 'applesauce-core/helpers';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  createPublicIngress,
  publicIngressStats
} from '../../src/lib/nostr/ingress.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../../src/lib/nostr/verified-envelope.ts';
import {
  createObservationJournal,
  createObservationContext,
  recordPublicObservation,
  exactPublicObservation,
  publicObservations,
  observationJournalStats,
  closeObservationJournal
} from '../../src/lib/catalog/observations.ts';
import {
  createPublicRequestResult,
  handlePublicRequestMessage,
  publicRequestSnapshot,
  disposePublicRequestResult
} from '../../src/lib/nostr/request-result.ts';
const origins = ['wss://one.example.org', 'wss://two.example.org'];
const policy = validateRelayPolicy(
  JSON.stringify({
    schemaVersion: 1,
    public: origins.map((origin) => ({
      origin,
      read: true,
      write: false,
      nip50: false
    })),
    inbox: [],
    postingEnabled: false,
    messagingEnabled: false,
    operatorDenylist: []
  })
)!;
const corpus = JSON.parse(
  readFileSync(
    new URL(
      '../../../contracts/interop/food_availability/corpus.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
const wires = corpus.vectors.find((v) => v.id.endsWith('_032'))!.signed_wires;
function proof(raw: string) {
  const result = verifyEnvelope(raw);
  if (!result.ok) throw new Error('Invalid signed test vector');
  return result.value;
}
const current = proof(wires.current);
const previous = proof(wires.previous);
const currentEvent = () =>
  JSON.parse(wires.current) as NonNullable<
    ReturnType<typeof verifiedEnvelopeSnapshot>
  >;
function setup() {
  const journal = createObservationJournal(policy);
  const ingress = createPublicIngress();
  const result = createPublicRequestResult(policy, ingress, journal);
  return { journal, ingress, result };
}
describe('exact public source observations', () => {
  it('binds every request in a journal to one shared admission owner', () => {
    const { journal, ingress } = setup();
    expect(() =>
      createPublicRequestResult(policy, createPublicIngress(), journal)
    ).toThrow('public_request_ingress_changed');
    expect(publicIngressStats(ingress).deliveries).toBe(0);
    expect(observationJournalStats(journal).rows).toBe(0);
  });
  it('rejects a different source manifest before request observation creation', () => {
    const { journal, ingress } = setup();
    const different = validateRelayPolicy(
      JSON.stringify({
        schemaVersion: 1,
        public: [
          { origin: origins[0], read: true, write: false, nip50: false }
        ],
        inbox: [],
        postingEnabled: false,
        messagingEnabled: false,
        operatorDenylist: []
      })
    )!;
    expect(() =>
      createPublicRequestResult(different, ingress, journal)
    ).toThrow('request_observation_policy_changed');
    expect(publicIngressStats(ingress).deliveries).toBe(0);
    expect(observationJournalStats(journal).rows).toBe(0);
  });
  it('does not cache authentic events outside the shared approved public inventory', () => {
    const { journal, ingress, result } = setup();
    const raw = corpus.vectors.find((row) => row.id.endsWith('_019'))!
      .signed_wires.event;
    expect(verifyEnvelope(raw).ok).toBe(true);
    expect(
      handlePublicRequestMessage(result, {
        type: 'EVENT',
        from: origins[0],
        id: 'a',
        event: JSON.parse(raw) as ReturnType<typeof currentEvent>
      }).status
    ).toBe('not_public');
    expect(
      handlePublicRequestMessage(result, {
        type: 'EVENT',
        from: origins[0],
        id: 'a',
        event: currentEvent()
      }).status
    ).toBe('accepted');
    expect(
      publicObservations(journal, publicRequestSnapshot(result).context)
    ).toHaveLength(1);
    expect(publicIngressStats(ingress).deliveries).toBe(2);
  });
  it('deduplicates only the exact event/source/request tuple', () => {
    const journal = createObservationJournal(policy);
    const first = createObservationContext(journal);
    const second = createObservationContext(journal);
    expect(recordPublicObservation(journal, first, origins[0], current)).toBe(
      true
    );
    expect(recordPublicObservation(journal, first, origins[0], current)).toBe(
      true
    );
    expect(recordPublicObservation(journal, first, origins[1], current)).toBe(
      true
    );
    expect(recordPublicObservation(journal, second, origins[0], current)).toBe(
      true
    );
    expect(publicObservations(journal, first)).toHaveLength(2);
    expect(publicObservations(journal, second)).toHaveLength(1);
    expect(observationJournalStats(journal).rows).toBe(3);
    (publicObservations(journal, first) as unknown as unknown[]).pop();
    expect(publicObservations(journal, first)).toHaveLength(2);
  });
  it('never derives exact receipt from actual SDK cross-version seen metadata', () => {
    const journal = createObservationJournal(policy);
    const context = createObservationContext(journal);
    const sdk = new EventStore({ keepOldVersions: false });
    const newer = currentEvent();
    const older = JSON.parse(wires.previous) as typeof newer;
    sdk.add(newer, origins[0]);
    sdk.add(older, origins[1]);
    expect(getSeenRelays(newer)?.has(origins[1])).toBe(true);
    recordPublicObservation(journal, context, origins[0], current);
    recordPublicObservation(journal, context, origins[1], previous);
    expect(exactPublicObservation(journal, context, newer.id, origins[0])).toBe(
      true
    );
    expect(exactPublicObservation(journal, context, newer.id, origins[1])).toBe(
      false
    );
    expect(exactPublicObservation(journal, context, older.id, origins[1])).toBe(
      true
    );
    sdk.dispose();
  });
  it('rejects forged proof, unapproved source and another journal context', () => {
    const journal = createObservationJournal(policy);
    const context = createObservationContext(journal);
    expect(
      recordPublicObservation(
        journal,
        context,
        origins[0],
        {} as typeof current
      )
    ).toBe(false);
    expect(
      recordPublicObservation(
        journal,
        context,
        'wss://hint.example.org',
        current
      )
    ).toBe(false);
    const other = createObservationJournal(policy);
    const alien = createObservationContext(other);
    expect(() =>
      recordPublicObservation(journal, alien, origins[0], current)
    ).toThrow('observation_context_invalid');
    expect(() => publicObservations({} as typeof journal, context)).toThrow(
      'observation_journal_invalid'
    );
    expect(observationJournalStats(journal).rows).toBe(0);
  });
  it('charges duplicates while retaining their freshly verified exact source proof before store dedup', () => {
    const { journal, ingress, result } = setup();
    const first = handlePublicRequestMessage(result, {
      type: 'EVENT',
      from: origins[0],
      id: 'a',
      event: currentEvent()
    });
    const second = handlePublicRequestMessage(result, {
      type: 'EVENT',
      from: origins[1] + '/',
      id: 'b',
      event: currentEvent()
    });
    expect(first.status).toBe('accepted');
    expect(second.status).toBe('duplicate');
    if (second.status === 'duplicate')
      expect(verifiedEnvelopeSnapshot(second.value)?.id).toBe(
        currentEvent().id
      );
    const context = publicRequestSnapshot(result).context;
    expect(
      exactPublicObservation(journal, context, currentEvent().id, origins[0])
    ).toBe(true);
    expect(
      exactPublicObservation(journal, context, currentEvent().id, origins[1])
    ).toBe(true);
    expect(publicIngressStats(ingress).deliveries).toBe(2);
    expect(publicIngressStats(ingress).chargedBytes).toBeGreaterThan(0);
  });
  it('retains valid evidence across a malformed candidate and failing source', () => {
    const { journal, ingress, result } = setup();
    expect(
      handlePublicRequestMessage(result, {
        type: 'EVENT',
        from: origins[0],
        id: 'a',
        event: { ...currentEvent(), sig: '0'.repeat(128) }
      }).status
    ).toBe('rejected');
    expect(
      handlePublicRequestMessage(result, {
        type: 'EVENT',
        from: origins[0],
        id: 'a',
        event: currentEvent()
      }).status
    ).toBe('accepted');
    handlePublicRequestMessage(result, {
      type: 'EOSE',
      from: origins[0],
      id: 'a'
    });
    handlePublicRequestMessage(result, {
      type: 'ERROR',
      from: origins[1],
      error: new Error('HC_TEST_ONLY_RAW_PRIVATE_ERROR')
    });
    const snapshot = publicRequestSnapshot(result);
    expect(snapshot.coverage).toBe('partial');
    expect(snapshot.definitiveAbsence).toBe(false);
    expect(publicObservations(journal, snapshot.context)).toHaveLength(1);
    expect(publicIngressStats(ingress).deliveries).toBe(2);
    expect(JSON.stringify(snapshot)).not.toContain(
      'HC_TEST_ONLY_RAW_PRIVATE_ERROR'
    );
  });
  it('does not inspect or expose arbitrary relay reasons or error objects', () => {
    const { result } = setup();
    let reads = 0;
    handlePublicRequestMessage(result, {
      type: 'CLOSED',
      from: origins[0],
      id: 'a',
      get reason(): string {
        reads++;
        throw new Error('Untrusted relay text');
      }
    });
    handlePublicRequestMessage(result, {
      type: 'ERROR',
      from: origins[1],
      get error() {
        reads++;
        throw new Error('Untrusted error object');
      }
    });
    expect(reads).toBe(0);
    const snapshot = publicRequestSnapshot(result);
    expect(snapshot.sources.map((row) => row.state)).toEqual([
      'closed',
      'error'
    ]);
    expect(JSON.stringify(snapshot)).not.toContain('Untrusted');
  });
  it('ignores unknown source hints without letting them become observation authority', () => {
    const { journal, ingress, result } = setup();
    expect(
      handlePublicRequestMessage(result, {
        type: 'EVENT',
        from: 'wss://hint.example.org',
        id: 'a',
        event: currentEvent()
      }).status
    ).toBe('source_unknown');
    const snapshot = publicRequestSnapshot(result);
    expect(publicObservations(journal, snapshot.context)).toEqual([]);
    expect(publicIngressStats(ingress).deliveries).toBe(1);
    expect(JSON.stringify(snapshot)).not.toContain('hint.example.org');
  });
  it('bounds journal rows across all request contexts at the approved delivery cap', () => {
    const journal = createObservationJournal(policy);
    for (let i = 0; i < 2000; i++)
      expect(
        recordPublicObservation(
          journal,
          createObservationContext(journal),
          origins[0],
          current
        )
      ).toBe(true);
    expect(
      recordPublicObservation(
        journal,
        createObservationContext(journal),
        origins[0],
        current
      )
    ).toBe(false);
    expect(observationJournalStats(journal)).toEqual({
      rows: 2000,
      closed: false
    });
  });
  it('cannot turn a degraded empty request or bounded EOSE into definitive absence', () => {
    const { result } = setup();
    handlePublicRequestMessage(result, {
      type: 'EOSE',
      from: origins[0],
      id: 'a'
    });
    expect(publicRequestSnapshot(result).coverage).toBe('partial');
    handlePublicRequestMessage(result, {
      type: 'EOSE',
      from: origins[1],
      id: 'b'
    });
    expect(publicRequestSnapshot(result).coverage).toBe('bounded-eose');
    expect(publicRequestSnapshot(result).definitiveAbsence).toBe(false);
  });
  it('disposes only its request and preserves another request and valid observations', () => {
    const { journal, ingress, result } = setup();
    const other = createPublicRequestResult(policy, ingress, journal);
    handlePublicRequestMessage(result, {
      type: 'EVENT',
      from: origins[0],
      id: 'a',
      event: currentEvent()
    });
    const context = publicRequestSnapshot(result).context;
    disposePublicRequestResult(result);
    expect(
      handlePublicRequestMessage(result, {
        type: 'EVENT',
        from: origins[1],
        id: 'b',
        event: currentEvent()
      }).status
    ).toBe('inactive');
    expect(publicObservations(journal, context)).toHaveLength(1);
    expect(
      handlePublicRequestMessage(other, {
        type: 'EVENT',
        from: origins[1],
        id: 'b',
        event: currentEvent()
      }).status
    ).toBe('duplicate');
    expect(
      publicObservations(journal, publicRequestSnapshot(other).context)
    ).toHaveLength(1);
  });
  it('keeps snapshots detached and explicit journal closure terminal', () => {
    const { journal, result } = setup();
    handlePublicRequestMessage(result, {
      type: 'EVENT',
      from: origins[0],
      id: 'a',
      event: currentEvent()
    });
    const snapshot = publicRequestSnapshot(result);
    (snapshot.sources as unknown as unknown[]).pop();
    expect(publicRequestSnapshot(result).sources).toHaveLength(2);
    closeObservationJournal(journal);
    expect(publicObservations(journal, snapshot.context)).toEqual([]);
    expect(
      recordPublicObservation(journal, snapshot.context, origins[0], current)
    ).toBe(false);
    expect(() => createObservationContext(journal)).toThrow(
      'observations_closed'
    );
  });
});

describe('qualified sample observation scope', () => {
  const samplePolicy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: origins.map((origin, index) => ({
        origin,
        read: true,
        write: false,
        nip50: index === 0
      })),
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  )!;
  it('bounds observations to the selected qualified source while charging unexpected deliveries', () => {
    const journal = createObservationJournal(samplePolicy),
      ingress = createPublicIngress();
    const result = createPublicRequestResult(
      samplePolicy,
      ingress,
      journal,
      origins[0]
    );
    expect(
      handlePublicRequestMessage(result, {
        type: 'EVENT',
        id: 'sample',
        from: origins[1],
        event: currentEvent()
      }).status
    ).toBe('source_unknown');
    expect(publicIngressStats(ingress).deliveries).toBe(1);
    expect(publicRequestSnapshot(result).sources).toHaveLength(1);
    expect(observationJournalStats(journal).rows).toBe(0);
    handlePublicRequestMessage(result, {
      type: 'EOSE',
      id: 'sample',
      from: origins[0]
    });
    expect(publicRequestSnapshot(result).coverage).toBe('bounded-eose');
    disposePublicRequestResult(result);
    closeObservationJournal(journal);
  });
  it('rejects an unqualified source before observation context allocation', () => {
    const journal = createObservationJournal(samplePolicy),
      ingress = createPublicIngress(),
      before = observationJournalStats(journal);
    expect(() =>
      createPublicRequestResult(samplePolicy, ingress, journal, origins[1])
    ).toThrow('nip50_source_unqualified');
    expect(observationJournalStats(journal)).toEqual(before);
    expect(publicIngressStats(ingress).deliveries).toBe(0);
    closeObservationJournal(journal);
  });
});
