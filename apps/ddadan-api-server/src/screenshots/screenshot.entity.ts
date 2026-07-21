import { Column, Entity, Index, JoinColumn, ManyToOne } from 'typeorm';
import { BaseEntity } from '../common/base.entity';
import { Device } from '../devices/device.entity';

@Entity('screenshots')
@Index(['deviceId'])
export class Screenshot extends BaseEntity {
  @Column({ type: 'integer' })
  deviceId!: number;

  @ManyToOne(() => Device, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'deviceId' })
  device?: Device;

  /** Path relative to the assets public root, e.g. `screenshots/1699-abc.jpg`. */
  @Column({ type: 'varchar', length: 500 })
  filePath!: string;

  @Column({ type: 'varchar', length: 100, nullable: true })
  mimeType!: string | null;

  @Column({ type: 'integer', default: 0 })
  sizeBytes!: number;
}
