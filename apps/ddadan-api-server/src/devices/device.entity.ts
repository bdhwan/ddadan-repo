import { Column, Entity, Index, JoinColumn, ManyToOne } from 'typeorm';
import { BaseEntity } from '../common/base.entity';
import { Store } from '../stores/store.entity';

export type DeviceStatus = 'unregistered' | 'online' | 'offline';

@Entity('devices')
@Index(['storeId'])
@Index(['hardwareId'], { unique: true })
export class Device extends BaseEntity {
  @Column({ type: 'integer', nullable: true })
  storeId!: number | null;

  @ManyToOne(() => Store, { onDelete: 'CASCADE', nullable: true })
  @JoinColumn({ name: 'storeId' })
  store?: Store | null;

  @Column({ type: 'varchar', length: 128 })
  hardwareId!: string;

  @Column({ type: 'varchar', length: 200, nullable: true })
  name!: string | null;

  @Column({ type: 'varchar', length: 32, default: 'unregistered' })
  status!: DeviceStatus;

  @Column({ type: 'datetime', precision: 6, nullable: true })
  lastSeenAt!: Date | null;

  @Column({ type: 'varchar', length: 64, nullable: true })
  appVersion!: string | null;

  // 최신 자원 텔레메트리 스냅샷(워치독이 주기 전송).
  @Column({ type: 'float', nullable: true })
  lastCpuPercent!: number | null;

  @Column({ type: 'integer', nullable: true })
  lastRamUsedMb!: number | null;

  @Column({ type: 'integer', nullable: true })
  lastRamTotalMb!: number | null;

  @Column({ type: 'float', nullable: true })
  lastDiskUsedPercent!: number | null;
}
