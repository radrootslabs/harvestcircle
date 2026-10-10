// HC_TEST_ONLY_QUALIFICATION_PORT: genuine production owners/native IndexedDB
// with controlled exercised inbox observations, never live extension/relay Q.
import { mount, unmount } from 'svelte';
import ListingEditor from '../../../src/lib/components/ListingEditor.svelte';
import { makeSetupFixture, fixturePolicy, inbox } from './inbox-setup.ts';
import { inspectNative } from './listing-editor.ts';
import {
  createIdentityViewContext,
  mountIdentityView,
  connectIdentityView,
  probeIdentityViewMessaging,
  identityViewSession,
  closeIdentityView,
  disconnectIdentityView,
  IDENTITY_VIEW_CONTEXT
} from '../../../src/lib/runtime/view-context.ts';
import {
  createInboxSetupView,
  checkInboxSetupView,
  unlockInboxSetupView,
  stopInboxSetupView,
  closeInboxSetupView
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import {
  openListingEditor,
  editListingEditor,
  listingEditorSnapshot,
  closeListingEditor,
  type ListingEditor as Editor
} from '../../../src/lib/publishing/listing-editor.ts';
import {
  openEditorReview,
  editorReviewSnapshot,
  closeEditorReview,
  type EditorReview
} from '../../../src/lib/publishing/editor-workflow.ts';
import type { PublicDraftForm } from '../../../src/lib/contracts/local-records.ts';
export async function createFixture(qualified = true) {
  const f = await makeSetupFixture([['relay', inbox]]),
    context = createIdentityViewContext();
  mountIdentityView(context);
  await connectIdentityView(context);
  await probeIdentityViewMessaging(context, 'reviewed_self_copy');
  const identity = identityViewSession(context);
  if (!identity) throw Error('original editor session unavailable');
  const setup = createInboxSetupView({
    identity,
    policy: fixturePolicy(),
    lookup: () => Promise.resolve(f.resolve().resolver),
    ...(qualified
      ? {
          observeAccess: (
            value: import('../../../src/lib/messaging/readiness.ts').OwnInboxAccessContext
          ) => ({
            ...value,
            readTargets: value.readTargets.slice(),
            archiveTargets: value.archiveTargets.slice(),
            receive: 'qualified_exercised' as const,
            archive: 'qualified_exercised' as const,
            current: () => true
          })
        }
      : {})
  });
  if (!setup) throw Error('original inbox setup unavailable');
  await checkInboxSetupView(setup);
  await unlockInboxSetupView(setup);
  let editor: Editor | undefined, review: EditorReview | undefined;
  let mounted: ReturnType<typeof mount> | undefined,
    target: HTMLElement | undefined;
  return {
    owner: f.owner,
    counts: f.counts,
    inspect: inspectNative,
    async open() {
      const result = await openListingEditor(context, { mode: 'new' });
      if (result.status !== 'ready') throw Error(result.reason);
      editor = result.editor;
      return listingEditorSnapshot(editor);
    },
    edit: (form: PublicDraftForm) =>
      editor ? editListingEditor(editor, form) : Promise.resolve(false),
    async review(form: PublicDraftForm, copied = false) {
      if (!editor) throw Error('original editor unavailable');
      const result = await openEditorReview(
        copied ? { ...editor } : editor,
        setup,
        form
      );
      if (result.status === 'review') review = result.review;
      return result.status === 'review'
        ? {
            status: result.status,
            snapshot: editorReviewSnapshot(result.review)
          }
        : result;
    },
    snapshot: (copied = false) =>
      review && editorReviewSnapshot(copied ? { ...review } : review),
    stop: () => stopInboxSetupView(setup),
    disconnect: () => disconnectIdentityView(context),
    render() {
      target = document.createElement('div');
      document.body.append(target);
      mounted = mount(ListingEditor, {
        target,
        props: { route: { mode: 'new' }, inboxController: setup },
        context: new Map([[IDENTITY_VIEW_CONTEXT, context]])
      });
    },
    async close() {
      if (mounted) await unmount(mounted);
      target?.remove();
      if (review) closeEditorReview(review);
      if (editor) closeListingEditor(editor);
      closeInboxSetupView(setup);
      closeIdentityView(context);
      f.close();
    }
  };
}
