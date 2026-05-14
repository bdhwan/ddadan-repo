import {
  Body,
  Controller,
  Get,
  Param,
  ParseIntPipe,
  Patch,
} from '@nestjs/common';
import { UsersService } from '../users/users.service';
import { AssignScreenDto, SetMonitorRotationDto, UpdateMonitorPositionDto } from './dto/monitor.dto';
import { MonitorsService } from './monitors.service';

@Controller()
export class MonitorsController {
  constructor(
    private readonly monitors: MonitorsService,
    private readonly users: UsersService,
  ) {}

  @Get('devices/:deviceId/monitors')
  list(@Param('deviceId', ParseIntPipe) deviceId: number) {
    return this.monitors.listForDevice(deviceId, this.users.getLocalUserId());
  }

  @Patch('monitors/:id/position')
  updatePosition(
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateMonitorPositionDto,
  ) {
    return this.monitors.updatePosition(
      id,
      this.users.getLocalUserId(),
      dto.positionX,
      dto.positionY,
    );
  }

  @Patch('monitors/:id/screen')
  assignScreen(
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: AssignScreenDto,
  ) {
    return this.monitors.assignScreen(
      id,
      this.users.getLocalUserId(),
      dto.screenId ?? null,
    );
  }

  @Patch('monitors/:id/rotation')
  setRotation(
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: SetMonitorRotationDto,
  ) {
    return this.monitors.setRotation(id, this.users.getLocalUserId(), dto);
  }
}
