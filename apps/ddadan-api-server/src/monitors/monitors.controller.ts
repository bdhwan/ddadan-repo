import {
  Body,
  Controller,
  Get,
  Param,
  ParseIntPipe,
  Patch,
} from '@nestjs/common';
import { CurrentUser } from '../auth/current-user.decorator';
import type { AuthContext } from '../auth/firebase-auth.guard';
import { AssignScreenDto, UpdateMonitorPositionDto } from './dto/monitor.dto';
import { MonitorsService } from './monitors.service';

@Controller()
export class MonitorsController {
  constructor(private readonly monitors: MonitorsService) {}

  @Get('devices/:deviceId/monitors')
  list(
    @CurrentUser() auth: AuthContext,
    @Param('deviceId', ParseIntPipe) deviceId: number,
  ) {
    return this.monitors.listForDevice(deviceId, auth.userId);
  }

  @Patch('monitors/:id/position')
  updatePosition(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateMonitorPositionDto,
  ) {
    return this.monitors.updatePosition(id, auth.userId, dto.positionX, dto.positionY);
  }

  @Patch('monitors/:id/screen')
  assignScreen(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: AssignScreenDto,
  ) {
    return this.monitors.assignScreen(id, auth.userId, dto.screenId);
  }
}
