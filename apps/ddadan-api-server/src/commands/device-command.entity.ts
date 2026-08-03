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

  /**
   * 박스가 이 명령을 가져간(pending 조회에 포함된) 최초 시각. 타임아웃 판정 기준이다.
   * null 이면 아직 아무도 안 가져간 것 — 박스가 꺼져 있을 수 있으니 만료시키지 않는다.
   */
  @Column({ type: 'datetime', precision: 6, nullable: true })
  dispatchedAt!: Date | null;

  @Column({ type: 'datetime', precision: 6, nullable: true })
  ackedAt!: Date | null;

  @Column({ type: 'text', nullable: true })
  result!: string | null;
}
