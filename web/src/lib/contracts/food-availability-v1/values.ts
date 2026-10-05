// Strict authored values from public Radroots Lib189 Food Availability v1.
// Tolerant inbound normalization belongs to the separate reader adapter.
export const foodUnits = Object.freeze([
  'g',
  'kg',
  'lb',
  'oz',
  'each',
  'dozen',
  'bunch',
  'punnet',
  'bag',
  'basket'
] as const);
export type FoodUnit = (typeof foodUnits)[number];

declare const decimalValue: unique symbol;
export type CanonicalFoodAmount = string & {
  readonly [decimalValue]: true;
};
declare const currencyValue: unique symbol;
export type FoodCurrency = string & { readonly [currencyValue]: true };

export function canonicalFoodAmount(
  value: unknown
): CanonicalFoodAmount | undefined {
  // Twenty-eight ASCII digits and at most one decimal point. Check length
  // before regex work; keep the exact string rather than convert to Number.
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    value.length > 29 ||
    !/^(?:0|[1-9][0-9]*)(?:\.[0-9]*[1-9])?$/u.test(value)
  )
    return undefined;
  const digits = value.length - (value.includes('.') ? 1 : 0);
  if (digits > 28) return undefined;
  return value as CanonicalFoodAmount;
}

export function foodCurrency(value: unknown): FoodCurrency | undefined {
  if (
    typeof value !== 'string' ||
    value.length !== 3 ||
    !/^[A-Z]{3}$/u.test(value)
  )
    return undefined;
  return value as FoodCurrency;
}

export function foodUnit(value: unknown): FoodUnit | undefined {
  if (
    typeof value !== 'string' ||
    value.length > 6 ||
    !foodUnits.includes(value as FoodUnit)
  )
    return undefined;
  return value as FoodUnit;
}

export type FoodPrice = Readonly<{
  amount: CanonicalFoodAmount;
  currency: FoodCurrency;
  unit: FoodUnit;
}>;
export function foodPrice(
  amountValue: unknown,
  currencyInput: unknown,
  unitInput: unknown
): FoodPrice | undefined {
  const amount = canonicalFoodAmount(amountValue);
  const currency = foodCurrency(currencyInput);
  const unit = foodUnit(unitInput);
  if (amount === undefined || currency === undefined || unit === undefined)
    return undefined;
  return { amount, currency, unit };
}

export type FoodQuantity = Readonly<{
  amount: CanonicalFoodAmount;
  unit: FoodUnit;
}>;
export function foodQuantity(
  amountValue: unknown,
  unitInput: unknown
): FoodQuantity | undefined {
  const amount = canonicalFoodAmount(amountValue);
  const unit = foodUnit(unitInput);
  // Canonical zero has exactly one spelling. Missing/invalid data remains
  // absent; it is never converted into zero inventory.
  if (amount === undefined || amount === '0' || unit === undefined)
    return undefined;
  return { amount, unit };
}
