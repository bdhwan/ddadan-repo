import {
  Body,
  Controller,
  Delete,
  Get,
  HttpCode,
  Param,
  ParseIntPipe,
  Patch,
  Post,
} from '@nestjs/common';
import { UsersService } from '../users/users.service';
import { DevicesService } from './devices.service';
import {
  CheckDeviceDto,
  HeartbeatDto,
  RegisterDeviceDto,
  UpdateDeviceDto,
} from './dto/device.dto';

@Controller()
export class DevicesController {
  constructor(
    private readonly devices: DevicesService,
    private readonly users: UsersService,
  ) {}

  @Post('devices/check')
  check(@Body() dto: CheckDeviceDto) {
    return this.devices.checkByHardwareId(dto.hardwareId, dto.monitors);
  }

  @Post('devices/heartbeat')
  heartbeat(@Body() dto: HeartbeatDto) {
    return this.devices.heartbeat(dto);
  }

  @Post('devices')
  register(@Body() dto: RegisterDeviceDto) {
    return this.devices.register(this.users.getLocalUserId(), dto);
  }

  @Get('stores/:storeId/devices')
  listForStore(@Param('storeId', ParseIntPipe) storeId: number) {
    return this.devices.listForStore(storeId, this.users.getLocalUserId());
  }

  @Patch('devices/:id')
  rename(
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateDeviceDto,
  ) {
    return this.devices.rename(id, this.users.getLocalUserId(), dto.name ?? '');
  }

  @Delete('devices/:id')
  @HttpCode(204)
  async unregister(@Param('id', ParseIntPipe) id: number) {
    await this.devices.unregister(id, this.users.getLocalUserId());
  }

  @Get('devices')
  listAll() {
    return this.devices.listAllForOwner(this.users.getLocalUserId());
  }

  @Get('devices/:id')
  get(@Param('id', ParseIntPipe) id: number) {
    return this.devices.getViewForOwner(id, this.users.getLocalUserId());
  }
}
