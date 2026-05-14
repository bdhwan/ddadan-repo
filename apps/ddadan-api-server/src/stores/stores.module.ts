import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { UsersModule } from '../users/users.module';
import { Store } from './store.entity';
import { StoresController } from './stores.controller';
import { StoresService } from './stores.service';

@Module({
  imports: [TypeOrmModule.forFeature([Store]), UsersModule],
  providers: [StoresService],
  controllers: [StoresController],
  exports: [StoresService],
})
export class StoresModule {}
