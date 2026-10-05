export type ClassifiedPartition =
  | 'focused_food_availability'
  | 'operational_listing'
  | 'generic_nip99'
  | 'ambiguous';
// Raw tag-name presence counts even when a marker has no value.
// This pure classifier alone makes no event-verification or admission claim.
export function markerPartition(
  tags: readonly (readonly string[])[]
): ClassifiedPartition {
  let focused = false;
  let operational = false;
  for (const tag of tags) {
    if (tag[0] === 'radroots:price_unit' || tag[0] === 'radroots:quantity')
      focused = true;
    if (
      tag[0] === 'radroots:primary_bin' ||
      tag[0] === 'radroots:bin' ||
      tag[0] === 'radroots:price'
    )
      operational = true;
    if (focused && operational) return 'ambiguous';
  }
  return focused
    ? 'focused_food_availability'
    : operational
      ? 'operational_listing'
      : 'generic_nip99';
}
