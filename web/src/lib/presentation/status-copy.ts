// Presentation facts supplied by an admitted controller, not domain receipts.
export type OperationFact =
  | 'saved'
  | 'save-failed'
  | 'unsaved'
  | 'encrypted-saved'
  | 'preparing'
  | 'sending'
  | 'partial'
  | 'extension-pending'
  | 'needs-action'
  | 'stopped'
  | 'local-read'
  | 'unknown';
export type StatusCopy = Readonly<{
  tone: 'info' | 'warning' | 'error';
  text: string;
}>;
export type TargetReceipt = Readonly<{
  eventId: string;
  destination: string;
  role: 'public' | 'recipient' | 'archive';
  outcome: 'pending' | 'accepted' | 'refused' | 'timed-out' | 'unknown';
  readback: 'confirmed' | 'not-observed' | 'unknown';
}>;
export function statusCopy(fact: OperationFact): StatusCopy {
  switch (fact) {
    case 'saved':
      return { tone: 'info', text: 'Saved in this browser.' };
    case 'save-failed':
      return {
        tone: 'error',
        text: 'Your latest changes were not saved. Keep this page open and try again.'
      };
    case 'unsaved':
      return {
        tone: 'warning',
        text: 'Not yet saved. Unsent text is lost when this page closes.'
      };
    case 'encrypted-saved':
      return { tone: 'info', text: 'Saved encrypted in this browser.' };
    case 'preparing':
      return { tone: 'info', text: 'Preparing encryption.' };
    case 'sending':
      return { tone: 'info', text: 'Sending.' };
    case 'partial':
      return {
        tone: 'warning',
        text: 'Delivery is partial. Check the individual target outcomes.'
      };
    case 'extension-pending':
      return {
        tone: 'info',
        text: 'Waiting for your extension. Another approval may be needed.'
      };
    case 'needs-action':
      return {
        tone: 'warning',
        text: 'Needs action. Keep the original operation for recovery.'
      };
    case 'stopped':
      return {
        tone: 'warning',
        text: 'Stopped scheduling further work. Accepted or uncertain effects are not reversed.'
      };
    case 'local-read':
      return {
        tone: 'info',
        text: 'Marked read in this browser only. The sender is not notified.'
      };
    default:
      return {
        tone: 'warning',
        text: 'The outcome is unknown. Keep the original operation for reconciliation.'
      };
  }
}
export function receiptCopy(
  receipt: TargetReceipt
): StatusCopy & Readonly<{ readback: string }> {
  const subject =
    receipt.role === 'recipient'
      ? "The recipient's inbox relay"
      : receipt.role === 'archive'
        ? 'The sender archive relay'
        : receipt.role === 'public'
          ? 'The public listing relay'
          : undefined;
  let text = 'The target outcome is unknown.';
  let tone: StatusCopy['tone'] = 'warning';
  if (subject)
    switch (receipt.outcome) {
      case 'accepted':
        text = subject + ' accepted the event.';
        tone = 'info';
        break;
      case 'refused':
        text = subject + ' refused the event. Needs action.';
        tone = 'error';
        break;
      case 'timed-out':
        text =
          subject +
          ' did not respond before the deadline. Acceptance is unknown.';
        break;
      case 'pending':
        text = subject + ' outcome is pending.';
        break;
      default:
        text =
          receipt.role === 'recipient'
            ? "We could not confirm whether the recipient's inbox accepted this message."
            : subject + ' acceptance is unknown.';
    }
  const readback =
    receipt.readback === 'confirmed'
      ? 'Exact read-back is confirmed for this event and target.'
      : receipt.readback === 'not-observed'
        ? 'Exact read-back was not observed for this event and target.'
        : 'Exact read-back is unknown.';
  return { tone, text, readback };
}
