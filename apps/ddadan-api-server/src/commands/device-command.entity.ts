import { Column, Entity, Index, JoinColumn, ManyToOne } from 'typeorm';
import { BaseEntity } from '../common/base.entity';
import { Device } from '../devices/device.entity';

export type CommandType =
  | 'reboot'
  | 'screenOn'
  | 'screenOff'
  | 'updateApp'
  | 'shell'
  | 'screenshot';

export type CommandStatus = 'pending' | 'done' | 'failed';

@Entity('device_commands')
@Index(['deviceId', 'status'])
export class DeviceCommand extends BaseEntity {
  @Column({ type: 'integer' })
  deviceId!: number;

  @ManyToOne(() => Device, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'deviceId' })
  device?: Device;

  @Column({ type: 'varchar', length: 32 })
  type!: CommandType;

  /** 명령 인자(예: shell 명령 문자열, updateApp 대상 applicationId). */
  @Column({ type: 'text', nullable: true })
  payload!: string | null;

  @Column({ type: 'varchar', length: 16, default: 'pending' })
  status!: CommandStatus;

  @Column({ type: 'datetime', precision: 6, nullable: true })
  ackedAt!: Date | null;

  @Column({ type: 'text', nullable: true })
  result!: string | null;
}
