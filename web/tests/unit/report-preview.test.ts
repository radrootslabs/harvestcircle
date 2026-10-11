import { describe, expect, it, vi } from 'vitest';
import {
  createGeneralReport,
  createListingReport,
  createConversationReport,
  reviewReportPreview,
  reportPreviewSnapshot,
  copyReportPreview,
  closeReportContext,
  type ReportContext,
  type ReportPreview
} from '../../src/lib/support/report.ts';
import type { ProductView } from '../../src/lib/catalog/product-view.ts';
import type { InboxView } from '../../src/lib/messaging/inbox-view.ts';

const selection = (notes: string, extra: Record<string, unknown> = {}) =>
  JSON.stringify({
    listingVersion: false,
    coordinate: false,
    peer: false,
    messages: [],
    notes,
    privateDisclosure: false,
    ...extra
  });

describe('HCP124 deliberately selected local report preview', () => {
  it('general notes are explicitly selected plain memory text, without hidden context', () => {
    const context = createGeneralReport();
    const preview = reviewReportPreview(
      context,
      selection('A deliberately typed problem description.'),
      'reviewed_report_preview'
    );
    expect(preview).toBeDefined();
    if (!preview) throw Error('explicit general preview');
    const text = reportPreviewSnapshot(preview);
    expect(text).toContain('A deliberately typed problem description.');
    expect(text).not.toMatch(/Peer:|Message:|Listing version:|Coordinate:/);
    closeReportContext(context);
    expect(reportPreviewSnapshot(preview)).toBeUndefined();
  });
  it('an empty unreviewed report cannot become a copyable preview', () => {
    const context = createGeneralReport();
    expect(
      reviewReportPreview(context, selection(''), 'reviewed_report_preview')
    ).toBeUndefined();
    expect(
      reviewReportPreview(context, selection('notes'), 'not reviewed')
    ).toBeUndefined();
    closeReportContext(context);
  });
  it('copied context, preview and private fields never reveal a transcript', () => {
    const fake = {
      kind: 'conversation',
      peer: 'a'.repeat(64),
      messages: [{ id: 'b'.repeat(64), content: 'PRIVATE_UNSELECTED_SENTINEL' }]
    };
    expect(
      reviewReportPreview(
        fake as unknown as ReportContext,
        selection('notes', {
          peer: true,
          messages: ['b'.repeat(64)],
          privateDisclosure: true
        }),
        'reviewed_report_preview'
      )
    ).toBeUndefined();
    expect(
      reportPreviewSnapshot({
        text: 'PRIVATE_UNSELECTED_SENTINEL'
      } as unknown as ReportPreview)
    ).toBeUndefined();
    expect(createListingReport(fake as unknown as ProductView)).toBeUndefined();
    expect(
      createConversationReport(
        fake as unknown as InboxView,
        '00000000-0000-4000-8000-000000000000'
      )
    ).toBeUndefined();
  });
  it('unknown, ill-formed and irrelevant selection fields do not silently attach data', () => {
    const context = createGeneralReport();
    for (const raw of [
      '{',
      selection('notes', { transcript: 'PRIVATE_SENTINEL' }),
      selection('notes', { messages: ['b'.repeat(64)] }),
      selection('notes', { peer: true }),
      selection('notes', { listingVersion: true }),
      selection('\ud800'),
      selection('notes', { privateDisclosure: 'yes' })
    ])
      expect(
        reviewReportPreview(context, raw, 'reviewed_report_preview')
      ).toBeUndefined();
    closeReportContext(context);
  });
  it('SSR preview preparation never acquires clipboard or private browser authority', async () => {
    vi.stubGlobal('window', undefined);
    let reads = 0;
    vi.stubGlobal('navigator', {
      get clipboard() {
        reads++;
        throw Error('SSR must not acquire clipboard');
      }
    });
    try {
      const context = createGeneralReport();
      const preview = reviewReportPreview(
        context,
        selection('Explicit SSR notes'),
        'reviewed_report_preview'
      );
      expect(preview).toBeDefined();
      if (!preview) throw Error('pure SSR preview');
      expect(await copyReportPreview(preview, 'reviewed_copy_report')).toBe(
        'unavailable'
      );
      expect(reads).toBe(0);
      closeReportContext(context);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
