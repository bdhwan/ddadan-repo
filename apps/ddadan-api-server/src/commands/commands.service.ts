import { Injectable, NotFoundException } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, Repository } from 'typeorm';
import { Device } from '../devices/device.entity';
import { DeviceCommand } from './device-command.entity';
import { AckCommandDto, CreateCommandDto } from './dto/command.dto';

@Injectable()
export class CommandsService {
  constructor(
    @InjectRepository(DeviceCommand)
    private readonly commands: Repository<DeviceCommand>,
    @InjectRepository(Device) private readonly devices: Repository<Device>,
  ) {}

  /** Admin: numeric device id로 명령 예약. */
  async enqueue(deviceId: number, dto: CreateCommandDto) {
    const device = await this.devices.findOne({
      where: { id: deviceId, deletedAt: IsNull() },
    });
    if (!device) throw new NotFoundException('Device not found');
    const cmd = this.commands.create({
      deviceId,
      type: dto.type,
      payload: dto.payload ?? null,
      status: 'pending',
    });
    return this.toView(await this.commands.save(cmd));
  }

  /** 박스: hardwareId로 대기 중(pending) 명령 조회. */
  async pendingForHardwareId(hardwareId: string) {
    const device = await this.devices.findOne({
      where: { hardwareId, deletedAt: IsNull() },
    });
    if (!device) throw new NotFoundException('Device not registered');
    const rows = await this.commands.find({
      where: { deviceId: device.id, status: 'pending' },
      order: { id: 'ASC' },
    });
    return rows.map((c) => this.toView(c));
  }

  /** 박스: 명령 실행 결과 보고. */
  async ack(hardwareId: string, id: number, dto: AckCommandDto) {
    const device = await this.devices.findOne({
      where: { hardwareId, deletedAt: IsNull() },
    });
    if (!device) throw new NotFoundException('Device not registered');
    const cmd = await this.commands.findOne({
      where: { id, deviceId: device.id },
    });
    if (!cmd) throw new NotFoundException('Command not found');
    cmd.status = dto.status;
    cmd.result = dto.result ?? null;
    cmd.ackedAt = new Date();
    return this.toView(await this.commands.save(cmd));
  }

  /** Admin: 명령 이력. */
  async history(deviceId: number, limit = 20) {
    const rows = await this.commands.find({
      where: { deviceId },
      order: { id: 'DESC' },
      take: Math.max(1, Math.min(limit, 100)),
    });
    return rows.map((c) => this.toView(c));
  }

  private toView(c: DeviceCommand) {
    return {
      id: c.id,
      type: c.type,
      payload: c.payload,
      status: c.status,
      result: c.result,
      createdAt: c.createdAt,
      ackedAt: c.ackedAt,
    };
  }
}
