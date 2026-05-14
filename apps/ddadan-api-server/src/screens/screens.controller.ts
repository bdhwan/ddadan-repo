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
import { UsersService } from '../users/users.service';
import {
  CreateScreenComponentDto,
  CreateScreenDto,
  UpdateScreenDto,
} from './dto/screen.dto';
import { ScreensService } from './screens.service';

@Controller()
export class ScreensController {
  constructor(
    private readonly screens: ScreensService,
    private readonly users: UsersService,
  ) {}

  @Get('screens')
  list(@Query('storeId') storeIdRaw?: string) {
    const storeId = storeIdRaw ? Number(storeIdRaw) : undefined;
    return this.screens.list(this.users.getLocalUserId(), storeId);
  }

  @Post('screens')
  create(@Body() dto: CreateScreenDto) {
    return this.screens.create(this.users.getLocalUserId(), dto);
  }

  @Get('screens/:id')
  get(@Param('id', ParseIntPipe) id: number) {
    return this.screens.findOwned(id, this.users.getLocalUserId());
  }

  @Patch('screens/:id')
  update(
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateScreenDto,
  ) {
    return this.screens.update(id, this.users.getLocalUserId(), dto);
  }

  @Delete('screens/:id')
  @HttpCode(204)
  async remove(@Param('id', ParseIntPipe) id: number) {
    await this.screens.remove(id, this.users.getLocalUserId());
  }

  @Get('screen-components')
  listComponents() {
    return this.screens.listComponents(this.users.getLocalUserId());
  }

  @Post('screen-components')
  createComponent(@Body() dto: CreateScreenComponentDto) {
    return this.screens.createComponent(this.users.getLocalUserId(), dto);
  }

  @Delete('screen-components/:id')
  @HttpCode(204)
  async removeComponent(@Param('id', ParseIntPipe) id: number) {
    await this.screens.removeComponent(id, this.users.getLocalUserId());
  }
}
