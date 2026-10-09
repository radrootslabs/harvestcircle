import {
  privateComposerSnapshot,
  discardPrivateComposer,
  type PrivateComposer
} from '../messaging/composer-state.ts';
declare const navigationBrand: unique symbol;
export type DirtyNavigation = Readonly<{ [navigationBrand]: true }>;
type Result =
  'allowed' | 'confirmation_required' | 'review_required' | 'invalid';
type Controller = {
  request(): Result;
  discard(review: unknown): Result;
  keep(): boolean;
  snapshot():
    | Readonly<{
        state: 'idle' | 'confirmation_required';
        copy: string;
        defaultAction: 'Keep editing';
        discardAction: 'Discard draft';
      }>
    | undefined;
  dirty(): boolean;
};
const navigations = new WeakMap<DirtyNavigation, Controller>();
// Shared route interception contract, with no URL/body/persistence/effect sink.
// Same-page validation and gates do not call this close contract. Mounted
// consumers own focus return and inline controls; native unload uses its own
// browser-provided confirmation, never an asynchronous save attempt.
export function captureDirtyNavigation(
  composer: PrivateComposer
): DirtyNavigation | undefined {
  if (typeof window === 'undefined' || !privateComposerSnapshot(composer))
    return undefined;
  let requestedRevision: number | undefined;
  function request(): Result {
    const value = privateComposerSnapshot(composer);
    if (!value) return 'invalid';
    if (!value.dirty) {
      requestedRevision = undefined;
      return 'allowed';
    }
    requestedRevision = value.revision;
    return 'confirmation_required';
  }
  function discard(review: unknown): Result {
    if (
      review !== 'reviewed_discard_private_text' ||
      requestedRevision === undefined
    )
      return 'invalid';
    const value = privateComposerSnapshot(composer),
      revision = requestedRevision;
    requestedRevision = undefined;
    if (!value) return 'invalid';
    if (value.revision !== revision) return 'review_required';
    return discardPrivateComposer(composer, revision, review)
      ? 'allowed'
      : 'invalid';
  }
  function keep() {
    if (!privateComposerSnapshot(composer)) return false;
    requestedRevision = undefined;
    return true;
  }
  function snapshot() {
    if (!privateComposerSnapshot(composer)) return undefined;
    return {
      state:
        requestedRevision === undefined
          ? ('idle' as const)
          : ('confirmation_required' as const),
      copy:
        requestedRevision === undefined
          ? ''
          : 'Discard unsaved private text? Keep editing to preserve it. An outstanding extension call remains pending until it settles.',
      defaultAction: 'Keep editing' as const,
      discardAction: 'Discard draft' as const
    };
  }
  const token = Object.freeze({}) as DirtyNavigation;
  navigations.set(token, {
    request,
    discard,
    keep,
    snapshot,
    dirty: () => privateComposerSnapshot(composer)?.dirty ?? false
  });
  return token;
}
export function requestPrivateNavigation(token: DirtyNavigation) {
  return navigations.get(token)?.request() ?? 'invalid';
}
export function confirmDiscardNavigation(
  token: DirtyNavigation,
  review: unknown
) {
  return navigations.get(token)?.discard(review) ?? 'invalid';
}
export function keepPrivateEditing(token: DirtyNavigation) {
  return navigations.get(token)?.keep() ?? false;
}
export function dirtyNavigationSnapshot(token: DirtyNavigation) {
  return navigations.get(token)?.snapshot();
}
export function privateNavigationBeforeUnload(
  token: DirtyNavigation,
  event: BeforeUnloadEvent
): boolean {
  const controller = navigations.get(token);
  if (typeof window === 'undefined' || !controller?.dirty()) return false;
  event.preventDefault();
  // The mounted consumer owns event registration and legacy returnValue.
  // This contract never receives or emits the private body or a destination.
  return true;
}
