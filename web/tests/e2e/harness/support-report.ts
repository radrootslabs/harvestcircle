// HC_TEST_ONLY_RELAY: original production views over real native IDB and
// controlled loopback. These parser/navigation values are not live support.
import { mount, unmount } from 'svelte';
import ReportPanel from '../../../src/lib/components/ReportPreview.svelte';
import SupportLink from '../../../src/lib/components/SupportLink.svelte';
import { createFixture as privateFixture } from './private-blocks.ts';
import { createFixture as publicFixture } from './public-hides.ts';
import {
  validateSupportPolicy,
  type SupportPolicy
} from '../../../src/lib/config/support.ts';
import {
  createGeneralReport,
  createConversationReport,
  createListingReport,
  reportContextSnapshot,
  reviewReportPreview,
  reportPreviewSnapshot,
  copyReportPreview,
  closeReportContext,
  type ReportContext,
  type ReportPreview
} from '../../../src/lib/support/report.ts';

function fixturePolicy(): SupportPolicy {
  const policy = validateSupportPolicy(
    JSON.stringify({
      schemaVersion: 1,
      owner: 'Controlled fixture operator',
      destination: 'https://support.operator.org/report'
    })
  );
  if (!policy) throw Error('controlled fixture support policy unavailable');
  return policy;
}
function panel(context: ReportContext) {
  const target = document.createElement('div');
  target.id = 'hcp124-report-fixture';
  document.body.append(target);
  const component = mount(ReportPanel, {
    target,
    props: { context, policy: fixturePolicy() }
  });
  return async () => {
    await unmount(component);
    closeReportContext(context);
    target.remove();
  };
}
export function createGeneralFixture() {
  const context = createGeneralReport();
  const close = panel(context);
  return { close, context, snapshot: () => reportContextSnapshot(context) };
}
export function createSupportFixture() {
  const target = document.createElement('div');
  target.id = 'hcp124-support-fixture';
  document.body.append(target);
  const component = mount(SupportLink, {
    target,
    props: { policy: fixturePolicy() }
  });
  return {
    async close() {
      await unmount(component);
      target.remove();
    }
  };
}
export async function createPrivateFixture() {
  const original = await privateFixture('contexts');
  let context: ReportContext | undefined,
    closePanel: (() => Promise<void>) | undefined,
    captured: ReportPreview | undefined;
  return {
    ...original,
    report() {
      context = createConversationReport(
        original.controller,
        original.conversationId
      );
      if (!context) throw Error('original unlocked report context unavailable');
      closePanel = panel(context);
      return reportContextSnapshot(context);
    },
    reportSnapshot: () => context && reportContextSnapshot(context),
    captureSelected() {
      if (!context) throw Error('original report not opened');
      captured = reviewReportPreview(
        context,
        JSON.stringify({
          listingVersion: false,
          coordinate: false,
          peer: false,
          messages: [original.firstRumor],
          notes: '',
          privateDisclosure: true
        }),
        'reviewed_report_preview'
      );
      if (!captured) throw Error('original selected preview unavailable');
      return reportPreviewSnapshot(captured);
    },
    captured: () => captured && reportPreviewSnapshot(captured),
    copyCaptured: () =>
      captured
        ? copyReportPreview(captured, 'reviewed_copy_report')
        : Promise.resolve('unavailable' as const),
    async close() {
      await closePanel?.();
      original.mode('normal');
      original.settle();
      original.releaseInventory();
      await original.close();
    }
  };
}
export async function createPublicFixture() {
  const original = await publicFixture();
  let context: ReportContext | undefined,
    closePanel: (() => Promise<void>) | undefined;
  return {
    ...original,
    report() {
      const view = original.view();
      if (!view) throw Error('original product view unavailable');
      context = createListingReport(view);
      if (!context) throw Error('original signed listing context unavailable');
      closePanel = panel(context);
      return reportContextSnapshot(context);
    },
    reportSnapshot: () => context && reportContextSnapshot(context),
    async close() {
      await closePanel?.();
      await original.close();
    }
  };
}
