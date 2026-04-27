import { Column, Entity, Index, JoinColumn, ManyToOne } from 'typeorm';
import { BaseEntity } from '../common/base.entity';
import { Device } from '../devices/device.entity';
import { Screen } from '../screens/screen.entity';

@Entity('monitors')
@Index(['deviceId'])
@Index(['deviceId', 'slot'], { unique: true })
export class Monitor extends BaseEntity {
  @Column({ type: 'bigint' })
  deviceId!: number;

  @ManyToOne(() => Device, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'deviceId' })
  device?: Device;

  @Column({ type: 'smallint' })
  slot!: number;

  @Column({ type: 'int', default: 1920 })
  resolutionW!: number;

  @Column({ type: 'int', default: 1080 })
  resolutionH!: number;

  @Column({ type: 'int', default: 0 })
  positionX!: number;

  @Column({ type: 'int', default: 0 })
  positionY!: number;

  @Column({ type: 'bigint', nullable: true })
  currentScreenId!: number | null;

  @ManyToOne(() => Screen, { onDelete: 'SET NULL', nullable: true })
  @JoinColumn({ name: 'currentScreenId' })
  currentScreen?: Screen | null;
}
