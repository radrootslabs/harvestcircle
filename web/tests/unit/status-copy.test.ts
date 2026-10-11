import { describe, it, expect } from 'vitest';
import {
  statusCopy,
  receiptCopy,
  type OperationFact
} from '../../src/lib/presentation/status-copy';
describe('finite evidence-specific presentation', () => {
  it('all target facts keep relay acceptance and exact readback distinct from person delivery or remote reading', () => {
    for (const role of ['public', 'recipient', 'archive'] as const)
      for (const outcome of [
        'pending',
        'accepted',
        'refused',
        'timed-out',
        'unknown'
      ] as const)
        for (const readback of [
          'confirmed',
          'not-observed',
          'unknown'
        ] as const) {
          const copy = receiptCopy({
            role,
            outcome,
            readback,
            eventId: 'event-A',
            destination: 'target-A'
          });
          expect(copy.text + ' ' + copy.readback).not.toMatch(
            /order placed|delivered to|person received|remote (?:seen|read)|guaranteed|complete sync/i
          );
          if (outcome !== 'accepted')
            expect(copy.text).not.toContain(' accepted the event.');
          if (outcome === 'accepted')
            expect(copy.text).toContain('relay accepted the event.');
          expect(copy.readback).toBe(
            readback === 'confirmed'
              ? 'Exact read-back is confirmed for this event and target.'
              : readback === 'not-observed'
                ? 'Exact read-back was not observed for this event and target.'
                : 'Exact read-back is unknown.'
          );
        }
  });
  it('never maps unsupported or unknown to success', () => {
    for (const state of ['unknown', 'new-future-state', null, undefined, 42])
      expect(statusCopy(state as OperationFact)).toEqual({
        tone: 'warning',
        text: 'The outcome is unknown. Keep the original operation for reconciliation.'
      });
  });
  it('maps local facts precisely', () => {
    expect(statusCopy('saved').text).toBe('Saved in this browser.');
    expect(statusCopy('encrypted-saved').text).toBe(
      'Saved encrypted in this browser.'
    );
    expect(statusCopy('unsaved').text).toContain('Unsent text is lost');
  });
  it('distinguishes each role and outcome', () => {
    for (const role of ['public', 'recipient', 'archive'] as const) {
      const values = [
        'accepted',
        'refused',
        'timed-out',
        'unknown',
        'pending'
      ] as const;
      const texts = values.map(
        (outcome) =>
          receiptCopy({
            role,
            outcome,
            readback: 'unknown',
            eventId: 'event-A',
            destination: 'target-A'
          }).text
      );
      expect(new Set(texts).size).toBe(values.length);
    }
    expect(
      receiptCopy({
        role: 'archive',
        outcome: 'accepted',
        readback: 'confirmed',
        eventId: 'event-A',
        destination: 'target-A'
      }).text
    ).toContain('sender archive');
    expect(
      receiptCopy({
        role: 'archive',
        outcome: 'accepted',
        readback: 'confirmed',
        eventId: 'event-A',
        destination: 'target-A'
      }).text
    ).not.toContain("recipient's inbox relay accepted");
  });
  it('does not promote exact readback into acceptance', () => {
    const copy = receiptCopy({
      role: 'recipient',
      outcome: 'unknown',
      readback: 'confirmed',
      eventId: 'event-A',
      destination: 'target-A'
    });
    expect(copy.text).toContain('could not confirm');
    expect(copy.readback).toContain('confirmed');
    expect(copy.tone).toBe('warning');
  });
  it('unknown runtime role/outcome/readback never crashes or becomes accepted', () => {
    expect(
      receiptCopy({
        role: 'future',
        outcome: 'future',
        readback: 'future',
        eventId: 'event-A',
        destination: 'target-A'
      } as never)
    ).toEqual({
      tone: 'warning',
      text: 'The target outcome is unknown.',
      readback: 'Exact read-back is unknown.'
    });
  });
  it('keeps input immutable and ignores unrelated seen metadata', () => {
    const fact = Object.freeze({
      role: 'recipient' as const,
      outcome: 'unknown' as const,
      readback: 'unknown' as const,
      eventId: 'event-A',
      destination: 'target-A',
      seenRelays: ['relay-A']
    });
    expect(receiptCopy(fact).text).toContain('could not confirm');
    expect(fact.outcome).toBe('unknown');
  });
  it('has no unsupported commercial or remote receipt promise', () => {
    for (const fact of [
      'saved',
      'save-failed',
      'unsaved',
      'encrypted-saved',
      'preparing',
      'sending',
      'partial',
      'extension-pending',
      'needs-action',
      'stopped',
      'local-read',
      'unknown'
    ] as const)
      expect(statusCopy(fact).text).not.toMatch(
        /order placed|delivered|remote read|reserved|guaranteed|permanent|global sync/i
      );
  });
});
