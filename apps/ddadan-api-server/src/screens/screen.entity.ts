import { Column, Entity, Index, JoinColumn, ManyToOne } from 'typeorm';
import { BaseEntity } from '../common/base.entity';
import { Store } from '../stores/store.entity';
import { User } from '../users/user.entity';

export interface ScreenLayoutItem {
  id: string;
  kind: 'image' | 'video' | 'text';
  assetId?: number | null;
  componentId?: number | null;
  text?: string;
  fontSize?: number;
  color?: string;
  background?: string;
  x: number;
  y: number;
  width: number;
  height: number;
  zIndex?: number;
  durationMs?: number;
}

export interface ScreenLayout {
  background?: string;
  items: ScreenLayoutItem[];
}

@Entity('screens')
@Index(['ownerUserId'])
@Index(['storeId'])
export class Screen extends BaseEntity {
  @Column({ type: 'bigint' })
  ownerUserId!: number;

  @ManyToOne(() => User, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'ownerUserId' })
  owner?: User;

  @Column({ type: 'bigint', nullable: true })
  storeId!: number | null;

  @ManyToOne(() => Store, { onDelete: 'SET NULL', nullable: true })
  @JoinColumn({ name: 'storeId' })
  store?: Store | null;

  @Column({ type: 'varchar', length: 200 })
  name!: string;

  @Column({ type: 'int', default: 1920 })
  width!: number;

  @Column({ type: 'int', default: 1080 })
  height!: number;

  @Column({ type: 'json' })
  layout!: ScreenLayout;
}
