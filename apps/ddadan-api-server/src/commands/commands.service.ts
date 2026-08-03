import { Injectable, NotFoundException } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, LessThan, Repository } from 'typeorm';
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
    // 처음 가져가는 명령에 발송 시각을 남긴다 — 타임아웃 스윕의 기준점.
    const now = new Date();
    const fresh = rows.filter((c) => c.dispatchedAt == null);
    if (fresh.length) {
      for (const c of fresh) c.dispatchedAt = now;
      await this.commands.save(fresh);
    }
    return rows.map((c) => this.toView(c));
  }

  /**
   * 박스가 가져갔지만 오래도록 ack 하지 않은 명령을 실패 처리한다.
   *
   * 박스에서 명령 하나가 끝나지 않으면(예: OTA 다운로드가 중복 실행돼 깨진 경우) 그 뒤의
   * 모든 명령이 pending 에 갇혀 원격 제어가 통째로 멎는다. 실제로 박스 2대가 이 상태가 됐고
   * 서버에는 이를 풀어줄 장치가 없었다. 만료시켜 두면 같은 작업을 다시 큐잉할 수 있다.
   *
   * dispatchedAt 이 null 인 것(= 아직 아무도 안 가져감)은 건드리지 않는다. 박스가 꺼져 있는
   * 동안 쌓아둔 명령은 켜지면 처리돼야 하기 때문이다.
   */
  async expireStaleCommands(timeoutMs: number): Promise<number> {
    const cutoff = new Date(Date.now() - timeoutMs);
    const stale = await this.commands.find({
      where: { status: 'pending', dispatchedAt: LessThan(cutoff) },
    });
    if (!stale.length) return 0;
    for (const c of stale) {
      c.status = 'failed';
      c.result = `timeout: no ack within ${Math.round(timeoutMs / 60000)}m`;
      c.ackedAt = new Date();
    }
    await this.commands.save(stale);
    return stale.length;
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
      dispatchedAt: c.dispatchedAt,
      createdAt: c.createdAt,
      ackedAt: c.ackedAt,
    };
  }
}
