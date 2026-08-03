import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { Cron, CronExpression } from '@nestjs/schedule';
import { AppConfig } from '../config/configuration';
import { CommandsService } from './commands.service';

/**
 * 응답 없는 원격 명령을 주기적으로 실패 처리한다.
 *
 * 박스는 pending 명령을 순서대로 실행하는데, 하나가 끝나지 않으면 그 뒤가 전부 막힌다.
 * (실제 사례: OTA 다운로드가 otaLoop 과 commandLoop 에서 중복 실행돼 APK 가 깨졌고,
 * 그 updateApp 이 완료되지 않아 screenshot/shell 까지 몇 시간째 pending 이었다.)
 * 서버가 만료시켜 주면 같은 작업을 다시 큐잉해 재시도할 수 있다.
 */
@Injectable()
export class CommandsSweeperService {
  private readonly logger = new Logger(CommandsSweeperService.name);

  constructor(
    private readonly commands: CommandsService,
    private readonly config: ConfigService<AppConfig, true>,
  ) {}

  @Cron(CronExpression.EVERY_MINUTE)
  async sweepStaleCommands() {
    const seconds = this.config.get('commands', { infer: true }).timeoutSeconds;
    const n = await this.commands.expireStaleCommands(seconds * 1000);
    if (n > 0) {
      this.logger.warn(`Expired ${n} command(s) with no ack within ${seconds}s`);
    }
  }
}
