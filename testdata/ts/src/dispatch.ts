import { Matcher, ExactMatcher, normalize } from '@matching/index';
export const MATCHER = Symbol('MATCHER');
export const provider = { provide: MATCHER, useClass: ExactMatcher };
export class Dispatch {
  constructor(private matcher: Matcher) {}
  run(value: string): number {
    normalize(value);
    return this.matcher.match(value);
  }
}
