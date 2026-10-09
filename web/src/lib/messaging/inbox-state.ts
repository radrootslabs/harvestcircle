import {
  LOCAL_PERSISTENCE_BUDGETS,
  PRIVATE_TRANSPORT_BUDGETS
} from '../config/budgets.ts';
export type OuterFact = Readonly<{ outerId: string; outerTime: number }>;
export type OuterCursor = Readonly<{
  source: string;
  until: number | undefined;
  seen: readonly string[];
  state: string;
  partial: boolean;
  historyComplete: false;
}>;
function facts(rows: readonly OuterFact[]) {
  for (const row of rows)
    if (
      !/^[0-9a-f]{64}$/.test(row.outerId) ||
      !Number.isSafeInteger(row.outerTime) ||
      row.outerTime < 0
    )
      throw Error('history_outer_metadata_invalid');
}
export function initialOuterCursor(
  source: string,
  rows: readonly OuterFact[]
): OuterCursor {
  facts(rows);
  const seen = rows
    .map((row) => row.outerId)
    .filter((id, index, all) => all.indexOf(id) === index);
  if (seen.length > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes)
    throw Error('history_metadata_capacity');
  return {
    source,
    until: rows.length
      ? Math.min(...rows.map((row) => row.outerTime))
      : undefined,
    seen,
    state: 'older_available',
    partial: false,
    historyComplete: false
  };
}
export function advanceOuterPage(
  cursor: OuterCursor,
  rows: readonly OuterFact[],
  deliveries: number,
  reason: string
): OuterCursor {
  facts(rows);
  if (!Number.isSafeInteger(deliveries) || deliveries < 0)
    throw Error('history_delivery_count_invalid');
  let seen = cursor.seen.slice();
  let until = cursor.until,
    newIds = 0,
    outside = false;
  for (const row of rows) {
    if (cursor.until !== undefined && row.outerTime > cursor.until) {
      outside = true;
      continue;
    }
    if (!seen.includes(row.outerId)) {
      if (seen.length >= LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes)
        throw Error('history_metadata_capacity');
      seen = seen.concat(row.outerId);
      newIds++;
    }
    until =
      until === undefined ? row.outerTime : Math.min(until, row.outerTime);
  }
  const capped = deliveries >= PRIVATE_TRANSPORT_BUDGETS.requestedPerRelay;
  const partial = reason !== 'complete' || outside || rows.length < deliveries;
  const state =
    reason === 'budget'
      ? 'capped'
      : reason !== 'complete' || outside
        ? 'partial'
        : capped
          ? newIds === 0 && until === cursor.until
            ? 'saturated'
            : 'capped'
          : newIds > 0
            ? 'older_available'
            : 'window_exhausted';
  return {
    source: cursor.source,
    until,
    seen: seen.slice(),
    state,
    partial,
    historyComplete: false
  };
}
