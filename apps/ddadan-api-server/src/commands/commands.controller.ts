import {
  Body,
  Controller,
  Get,
  Param,
  ParseIntPipe,
  Post,
  Query,
} from '@nestjs/common';
import { CommandsService } from './commands.service';
import { AckCommandDto, CreateCommandDto } from './dto/command.dto';

@Controller('devices')
export class CommandsController {
  constructor(private readonly commands: CommandsService) {}

  /** Admin: 명령 예약(numeric device id). */
  @Post(':id/commands')
  enqueue(
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: CreateCommandDto,
  ) {
    return this.commands.enqueue(id, dto);
  }

  /** Admin: 명령 이력. */
  @Get(':id/commands')
  history(
    @Param('id', ParseIntPipe) id: number,
    @Query('limit') limitRaw?: string,
  ) {
    return this.commands.history(id, limitRaw ? Number(limitRaw) : 20);
  }

  /** 박스: 대기 중 명령 폴링(hardwareId). */
  @Get(':hardwareId/commands/pending')
  pending(@Param('hardwareId') hardwareId: string) {
    return this.commands.pendingForHardwareId(hardwareId);
  }

  /** 박스: 명령 실행 결과 보고. */
  @Post(':hardwareId/commands/:cmdId/ack')
  ack(
    @Param('hardwareId') hardwareId: string,
    @Param('cmdId', ParseIntPipe) cmdId: number,
    @Body() dto: AckCommandDto,
  ) {
    return this.commands.ack(hardwareId, cmdId, dto);
  }
}
