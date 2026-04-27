import { Column, Entity, Index, JoinColumn, ManyToOne } from 'typeorm';
import { BaseEntity } from '../common/base.entity';
import { Store } from '../stores/store.entity';
import { User } from '../users/user.entity';

export type AssetType = 'image' | 'video' | 'text';

@Entity('assets')
@Index(['ownerUserId'])
@Index(['storeId'])
export class Asset extends BaseEntity {
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

  @Column({ type: 'varchar', length: 16 })
  type!: AssetType;

  @Column({ type: 'varchar', length: 200 })
  originalName!: string;

  @Column({ type: 'varchar', length: 500, nullable: true })
  filePath!: string | null;

  @Column({ type: 'varchar', length: 100, nullable: true })
  mimeType!: string | null;

  @Column({ type: 'bigint', default: 0 })
  sizeBytes!: number;

  @Column({ type: 'text', nullable: true })
  textContent!: string | null;

  @Column({ type: 'json', nullable: true })
  metadata!: Record<string, unknown> | null;
}
