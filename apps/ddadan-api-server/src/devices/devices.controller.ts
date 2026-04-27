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
  Query,
} from '@nestjs/common';
import { CurrentUser } from '../auth/current-user.decorator';
import type { AuthContext } from '../auth/firebase-auth.guard';
import { Public } from '../auth/public.decorator';
import { DevicesService } from './devices.service';
import {
  CheckDeviceDto,
  HeartbeatDto,
  RegisterDeviceDto,
  UpdateDeviceDto,
} from './dto/device.dto';

@Controller()
export class DevicesController {
  constructor(private readonly devices: DevicesService) {}

  @Public()
  @Post('devices/check')
  check(@Body() dto: CheckDeviceDto) {
    return this.devices.checkByHardwareId(dto.hardwareId, dto.monitors);
  }

  @Public()
  @Post('devices/heartbeat')
  heartbeat(@Body() dto: HeartbeatDto) {
    return this.devices.heartbeat(dto);
  }

  @Post('devices')
  register(
    @CurrentUser() auth: AuthContext,
    @Body() dto: RegisterDeviceDto,
  ) {
    return this.devices.register(auth.userId, dto);
  }

  @Get('stores/:storeId/devices')
  listForStore(
    @CurrentUser() auth: AuthContext,
    @Param('storeId', ParseIntPipe) storeId: number,
  ) {
    return this.devices.listForStore(storeId, auth.userId);
  }

  @Patch('devices/:id')
  rename(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateDeviceDto,
  ) {
    return this.devices.rename(id, auth.userId, dto.name ?? '');
  }

  @Delete('devices/:id')
  @HttpCode(204)
  async unregister(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
  ) {
    await this.devices.unregister(id, auth.userId);
  }

  @Get('devices/:id')
  get(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
  ) {
    return this.devices.getOwned(id, auth.userId).then(async (d) => {
      const list = await this.devices.listForStore(d.storeId!, auth.userId);
      return list.find((x) => x.id === id);
    });
  }
}
