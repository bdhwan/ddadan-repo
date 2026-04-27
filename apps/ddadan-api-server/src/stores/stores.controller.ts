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
import { CurrentUser } from '../auth/current-user.decorator';
import type { AuthContext } from '../auth/firebase-auth.guard';
import { CreateStoreDto, UpdateStoreDto } from './dto/store.dto';
import { StoresService } from './stores.service';

@Controller('stores')
export class StoresController {
  constructor(private readonly stores: StoresService) {}

  @Get()
  list(@CurrentUser() auth: AuthContext) {
    return this.stores.list(auth.userId);
  }

  @Post()
  create(@CurrentUser() auth: AuthContext, @Body() dto: CreateStoreDto) {
    return this.stores.create(auth.userId, dto);
  }

  @Get(':id')
  get(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
  ) {
    return this.stores.getOwned(id, auth.userId);
  }

  @Patch(':id')
  update(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateStoreDto,
  ) {
    return this.stores.update(id, auth.userId, dto);
  }

  @Delete(':id')
  @HttpCode(204)
  async remove(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
  ) {
    await this.stores.remove(id, auth.userId);
  }
}
