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
import {
  CreateScreenComponentDto,
  CreateScreenDto,
  UpdateScreenDto,
} from './dto/screen.dto';
import { ScreensService } from './screens.service';

@Controller()
export class ScreensController {
  constructor(private readonly screens: ScreensService) {}

  @Get('screens')
  list(
    @CurrentUser() auth: AuthContext,
    @Query('storeId') storeIdRaw?: string,
  ) {
    const storeId = storeIdRaw ? Number(storeIdRaw) : undefined;
    return this.screens.list(auth.userId, storeId);
  }

  @Post('screens')
  create(
    @CurrentUser() auth: AuthContext,
    @Body() dto: CreateScreenDto,
  ) {
    return this.screens.create(auth.userId, dto);
  }

  @Get('screens/:id')
  get(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
  ) {
    return this.screens.findOwned(id, auth.userId);
  }

  @Patch('screens/:id')
  update(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateScreenDto,
  ) {
    return this.screens.update(id, auth.userId, dto);
  }

  @Delete('screens/:id')
  @HttpCode(204)
  async remove(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
  ) {
    await this.screens.remove(id, auth.userId);
  }

  @Get('screen-components')
  listComponents(@CurrentUser() auth: AuthContext) {
    return this.screens.listComponents(auth.userId);
  }

  @Post('screen-components')
  createComponent(
    @CurrentUser() auth: AuthContext,
    @Body() dto: CreateScreenComponentDto,
  ) {
    return this.screens.createComponent(auth.userId, dto);
  }

  @Delete('screen-components/:id')
  @HttpCode(204)
  async removeComponent(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
  ) {
    await this.screens.removeComponent(id, auth.userId);
  }
}
