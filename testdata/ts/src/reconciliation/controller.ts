import { Controller, Post } from '@nestjs/common';
import { EventPattern } from '@nestjs/microservices';
import { ReconciliationService } from './service';
@Controller('statements')
export class ReconciliationController {
  constructor(private readonly service: ReconciliationService) {}
  @EventPattern('bank.statement.imported')
  imported(statement: string[]) { return this.service.reconcile(statement); }
  @Post('/reconcile')
  reconcile(statement: string[]) { return this.service.reconcile(statement); }
}
