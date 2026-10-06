import { MatchingService } from '../matching/service';
type Repository = { save(v: unknown): Promise<void>; findAll(): Promise<string[]> };
type Database = { transaction(cb: () => Promise<void>): Promise<void> };
type Publisher = { publish(topic: string, v: unknown): void };
export class ReconciliationService {
  constructor(private matchingService: MatchingService, private openItemRepository: Repository,
    private matchRepository: Repository, private dataSource: Database, private pubsub: Publisher) {}
  async reconcile(statement: string[]): Promise<void> {
    const openItems = await this.openItemRepository.findAll();
    if (openItems.length === 0) { return; }
    for (const item of statement) {
      switch (true) {
        case item === 'skip': continue;
        default: this.normalize(item);
      }
      await this.matchingService.match(item);
    }
    await this.dataSource.transaction(async () => {
      await this.matchRepository.save(statement);
      await this.openItemRepository.save(openItems);
    });
    this.pubsub.publish('reconciliation.completed', statement);
  }
  private normalize(item: string) { this.pubsub.publish('item.normalized', item); }
}
