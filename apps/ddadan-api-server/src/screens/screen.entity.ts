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
  fontUnit?: 'px' | 'vh';
  color?: string;
  background?: string;
  opacity?: number;
  fontWeight?: number;
  textAlign?: 'left' | 'center' | 'right';
  lineHeight?: number;
  /** 'menuLine' renders label + dot leader + price (textSecondary). */
  textVariant?: 'plain' | 'menuLine';
  textSecondary?: string;
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
  @Column({ type: 'integer' })
  ownerUserId!: number;

  @ManyToOne(() => User, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'ownerUserId' })
  owner?: User;

  @Column({ type: 'integer', nullable: true })
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
