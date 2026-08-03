import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { Device } from '../devices/device.entity';
import { CommandsController } from './commands.controller';
import { CommandsService } from './commands.service';
import { CommandsSweeperService } from './commands-sweeper.service';
import { DeviceCommand } from './device-command.entity';

@Module({
  imports: [TypeOrmModule.forFeature([DeviceCommand, Device])],
  providers: [CommandsService, CommandsSweeperService],
  controllers: [CommandsController],
})
export class CommandsModule {}
