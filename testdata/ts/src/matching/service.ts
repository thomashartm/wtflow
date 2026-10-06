export class MatchingService {
  constructor(private aiMatcherClient: { score(item: string): Promise<number> }) {}
  async match(item: string): Promise<void> {
    let attempts = 0;
    while (attempts < 3) {
      try { await this.aiMatcherClient.score(item); break; }
      catch (error) { attempts++; }
    }
  }
}
