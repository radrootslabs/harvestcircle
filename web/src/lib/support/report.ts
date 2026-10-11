import {
  productViewSnapshot,
  productViewPublisherProof,
  type ProductView
} from '../catalog/product-view.ts';
import { verifiedEnvelopeSnapshot } from '../nostr/verified-envelope.ts';
import {
  inboxThreadSnapshot,
  inboxReplyOwnership,
  subscribeInboxView,
  type InboxView
} from '../messaging/inbox-view.ts';

declare const contextBrand: unique symbol, previewBrand: unique symbol;
export type ReportContext = Readonly<{ [contextBrand]: true }>;
export type ReportPreview = Readonly<{ [previewBrand]: true }>;
export type ReportContextSnapshot = Readonly<
  | { kind: 'general' }
  | { kind: 'listing'; eventId: string; coordinate: string }
  | {
      kind: 'conversation';
      peer: string;
      messages: readonly Readonly<{
        id: string;
        content: string;
        createdAt: number;
      }>[];
    }
>;
type Selection = Readonly<{
  listingVersion: boolean;
  coordinate: boolean;
  peer: boolean;
  messages: readonly string[];
  notes: string;
  privateDisclosure: boolean;
}>;
type PreviewOwner = {
  text(): string | undefined;
  current(): boolean;
  retire(): void;
};
type ContextOwner = {
  read(): ReportContextSnapshot | undefined;
  close(deferNotification?: boolean): void;
  subscribe(listener: () => void): () => void;
  own(preview: PreviewOwner): () => void;
  retirePreviews(): void;
  listen(unsubscribe: () => void): void;
  notify(): void;
};
const contexts = new WeakMap<ReportContext, ContextOwner>();
const previews = new WeakMap<ReportPreview, PreviewOwner>();

function capture(read: () => ReportContextSnapshot | undefined): ReportContext {
  const token = Object.freeze({}) as ReportContext;
  let closed = false;
  let source = read;
  let unsubscribe = () => {};
  const listeners = new Map<symbol, () => void>();
  const retained = new Map<symbol, PreviewOwner>();
  const retirePreviews = () => {
    for (const previous of retained.values()) previous.retire();
  };
  const notify = () => {
    for (const listener of listeners.values()) listener();
  };
  const close = (deferNotification = false) => {
    if (closed) return;
    closed = true;
    source = () => undefined;
    unsubscribe();
    unsubscribe = () => {};
    retirePreviews();
    const pending = Array.from(listeners.values());
    listeners.clear();
    // Plaintext and authority retire synchronously. Only UI notification may
    // be deferred when a Svelte derivation discovers original-owner loss.
    const notifyClosed = () => {
      for (const listener of pending) listener();
    };
    if (deferNotification && pending.length) queueMicrotask(notifyClosed);
    else notifyClosed();
  };
  contexts.set(token, {
    read() {
      if (closed) return undefined;
      let snapshot: ReportContextSnapshot | undefined;
      try {
        snapshot = source();
      } catch {
        /* Original source unavailable. */
      }
      if (!snapshot) close(true);
      return snapshot;
    },
    close,
    subscribe(listener) {
      if (closed) return () => {};
      const id = Symbol();
      listeners.set(id, listener);
      return () => {
        listeners.delete(id);
      };
    },
    own(preview) {
      const id = Symbol();
      if (closed) preview.retire();
      else retained.set(id, preview);
      return () => {
        retained.delete(id);
      };
    },
    retirePreviews,
    listen(next) {
      if (closed) next();
      else unsubscribe = next;
    },
    notify
  });
  return token;
}
export function createGeneralReport(): ReportContext {
  return capture(() => ({ kind: 'general' }));
}
export function createListingReport(
  view: ProductView
): ReportContext | undefined {
  if (typeof window === 'undefined') return undefined;
  const proof = productViewPublisherProof(view);
  const event = proof && verifiedEnvelopeSnapshot(proof);
  if (!event || event.kind !== 30402) return undefined;
  try {
    const initial = productViewSnapshot(view);
    if (
      initial.eventId !== event.id ||
      initial.reference.pubkey !== event.pubkey
    )
      return undefined;
    const coordinate = `30402:${initial.reference.pubkey}:${initial.reference.identifier}`;
    return capture(() => {
      try {
        const current = productViewSnapshot(view);
        const currentProof = productViewPublisherProof(view);
        const signed = currentProof && verifiedEnvelopeSnapshot(currentProof);
        return current.eventId === event.id &&
          signed?.id === event.id &&
          `30402:${current.reference.pubkey}:${current.reference.identifier}` ===
            coordinate
          ? { kind: 'listing', eventId: event.id, coordinate }
          : undefined;
      } catch {
        return undefined;
      }
    });
  } catch {
    return undefined;
  }
}
export function createConversationReport(
  view: InboxView,
  id: string
): ReportContext | undefined {
  if (typeof window === 'undefined') return undefined;
  const initial = inboxThreadSnapshot(view, id);
  const first = initial.messages[0];
  const original = first && inboxReplyOwnership(view, id, first.data.rumorId);
  if (initial.status !== 'ready' || !original?.current()) return undefined;
  const peer = original.peer;
  const token = capture(() => {
    if (!original.current()) return undefined;
    const thread = inboxThreadSnapshot(view, id);
    if (thread.status !== 'ready') return undefined;
    if (
      thread.messages.some((row) => {
        const owned = inboxReplyOwnership(view, id, row.data.rumorId);
        return !owned?.current() || owned.peer !== peer;
      })
    )
      return undefined;
    const rows = thread.messages.map((row) => ({
      id: row.data.rumorId,
      content: row.data.content,
      createdAt: row.data.createdAt
    }));
    /* verified original rows only */
    return { kind: 'conversation', peer, messages: rows };
  });
  const owned = contexts.get(token)!;
  owned.listen(
    subscribeInboxView(view, () => {
      if (reportContextSnapshot(token)) owned.notify();
    })
  );
  return token;
}
export function reportContextSnapshot(
  token: ReportContext
): ReportContextSnapshot | undefined {
  return contexts.get(token)?.read();
}
export function subscribeReportContext(
  token: ReportContext,
  listener: () => void
): () => void {
  return contexts.get(token)?.subscribe(listener) ?? (() => {});
}
export function closeReportContext(token: ReportContext): void {
  contexts.get(token)?.close();
}
function selection(raw: unknown): Selection | undefined {
  if (
    typeof raw !== 'string' ||
    raw.length > 65536 ||
    !raw.isWellFormed() ||
    new TextEncoder().encode(raw).length > 65536
  )
    return undefined;
  try {
    const value: unknown = JSON.parse(raw);
    if (!value || typeof value !== 'object' || Array.isArray(value))
      return undefined;
    const row = value as Record<string, unknown>;
    let count = 0;
    for (const key in row) {
      if (
        ![
          'listingVersion',
          'coordinate',
          'peer',
          'messages',
          'notes',
          'privateDisclosure'
        ].includes(key)
      )
        return undefined;
      count++;
    }
    if (
      count !== 6 ||
      typeof row.listingVersion !== 'boolean' ||
      typeof row.coordinate !== 'boolean' ||
      typeof row.peer !== 'boolean' ||
      typeof row.privateDisclosure !== 'boolean' ||
      typeof row.notes !== 'string' ||
      !row.notes.isWellFormed() ||
      new TextEncoder().encode(row.notes).length > 4096 ||
      !Array.isArray(row.messages) ||
      row.messages.some(
        (value) => typeof value !== 'string' || !/^[0-9a-f]{64}$/.test(value)
      ) ||
      new Set(row.messages).size !== row.messages.length
    )
      return undefined;
    return {
      listingVersion: row.listingVersion,
      coordinate: row.coordinate,
      peer: row.peer,
      messages: row.messages as string[],
      notes: row.notes,
      privateDisclosure: row.privateDisclosure
    };
  } catch {
    return undefined;
  }
}
function selectedText(
  snapshot: ReportContextSnapshot,
  selected: Selection
): string | undefined {
  if (
    snapshot.kind !== 'listing' &&
    (selected.listingVersion || selected.coordinate)
  )
    return undefined;
  if (
    snapshot.kind !== 'conversation' &&
    (selected.peer || selected.messages.length)
  )
    return undefined;
  if (snapshot.kind === 'conversation') {
    if (
      (selected.peer || selected.messages.length || selected.notes) &&
      !selected.privateDisclosure
    )
      return undefined;
    if (
      selected.messages.some(
        (id) => !snapshot.messages.some((message) => message.id === id)
      )
    )
      return undefined;
  }
  const lines = [
    'HarvestCircle report',
    ...(snapshot.kind === 'listing' && selected.listingVersion
      ? ['Listing version: ' + snapshot.eventId]
      : []),
    ...(snapshot.kind === 'listing' && selected.coordinate
      ? ['Coordinate: ' + snapshot.coordinate]
      : []),
    ...(snapshot.kind === 'conversation' && selected.peer
      ? ['Authenticated peer: ' + snapshot.peer]
      : []),
    ...(snapshot.kind === 'conversation'
      ? snapshot.messages
          .filter((message) => selected.messages.includes(message.id))
          .map(
            (message) =>
              `Message: ${message.id}\nTime: ${message.createdAt}\n${message.content}`
          )
      : []),
    ...(selected.notes ? ['Notes:\n' + selected.notes] : [])
  ];
  if (lines.length === 1) return undefined;
  const text = lines.join('\n\n');
  // Conservative local-copy ceiling, not a message/transport quota. Refuse the
  // complete selection rather than silently truncating private report content.
  return new TextEncoder().encode(text).length <= 65536 ? text : undefined;
}
export function reviewReportPreview(
  token: ReportContext,
  raw: unknown,
  review: unknown
): ReportPreview | undefined {
  if (review !== 'reviewed_report_preview') return undefined;
  const owned = contexts.get(token),
    selected = selection(raw),
    snapshot = reportContextSnapshot(token);
  if (!owned || !selected || !snapshot) return undefined;
  const text = selectedText(snapshot, selected);
  if (!text) return undefined;
  // One current reviewed preview per context; obsolete selections do not form
  // a retained plaintext history and cannot remain copyable by stale handles.
  owned.retirePreviews();
  const preview = Object.freeze({}) as ReportPreview;
  let currentText: string | undefined = text;
  let chosen: Selection | undefined = selected;
  let forget = () => {};
  const record: PreviewOwner = {
    text: () => currentText,
    current() {
      const actual = reportContextSnapshot(token);
      return (
        !!actual &&
        chosen !== undefined &&
        currentText !== undefined &&
        selectedText(actual, chosen) === currentText
      );
    },
    retire() {
      currentText = undefined;
      chosen = undefined;
      forget();
      forget = () => {};
    }
  };
  forget = owned.own(record);
  previews.set(preview, record);
  return preview;
}
export function reportPreviewSnapshot(
  token: ReportPreview
): string | undefined {
  const record = previews.get(token);
  if (!record || !record.current()) {
    if (record) record.retire();
    return undefined;
  }
  return record.text();
}
export function closeReportPreview(token: ReportPreview): void {
  const record = previews.get(token);
  if (record) record.retire();
}
export async function copyReportPreview(
  token: ReportPreview,
  review: unknown
): Promise<'copied' | 'unavailable' | 'stopped'> {
  if (review !== 'reviewed_copy_report' || !previews.has(token))
    return 'unavailable';
  const text = reportPreviewSnapshot(token);
  if (text === undefined) return 'stopped';
  if (typeof window === 'undefined' || typeof navigator === 'undefined')
    return 'unavailable';
  try {
    const clipboard = navigator.clipboard,
      write =
        clipboard &&
        Reflect.get<Clipboard, 'writeText'>(clipboard, 'writeText');
    if (typeof write !== 'function') return 'unavailable';
    if (reportPreviewSnapshot(token) !== text) return 'stopped';
    await write.call(clipboard, text);
    return reportPreviewSnapshot(token) === text ? 'copied' : 'stopped';
  } catch {
    return reportPreviewSnapshot(token) === text ? 'unavailable' : 'stopped';
  }
}
