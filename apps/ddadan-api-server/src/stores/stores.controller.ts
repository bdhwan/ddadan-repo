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
import { CreateStoreDto, UpdateStoreDto } from './dto/store.dto';
import { StoresService } from './stores.service';

@Controller('stores')
export class StoresController {
  constructor(
    private readonly stores: StoresService,
    private readonly users: UsersService,
  ) {}

  @Get()
  list() {
    return this.stores.list(this.users.getLocalUserId());
  }

  @Post()
  create(@Body() dto: CreateStoreDto) {
    return this.stores.create(this.users.getLocalUserId(), dto);
  }

  @Get(':id')
  get(@Param('id', ParseIntPipe) id: number) {
    return this.stores.getOwned(id, this.users.getLocalUserId());
  }

  @Patch(':id')
  update(
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateStoreDto,
  ) {
    return this.stores.update(id, this.users.getLocalUserId(), dto);
  }

  @Delete(':id')
  @HttpCode(204)
  async remove(@Param('id', ParseIntPipe) id: number) {
    await this.stores.remove(id, this.users.getLocalUserId());
  }
}
