export interface Matcher { match(value: string): number; }
/** Exact equality matcher. */
export class ExactMatcher implements Matcher {
  /** Return exact match score. */
  match(value: string): number { return value === 'exact' ? 1 : 0; }
}
export class FuzzyMatcher implements Matcher {
  match(value: string): number { return value.length > 3 ? 1 : 0; }
}
export function normalize(value: string): string { return value.trim(); }
