import { Column, Entity, Index } from 'typeorm';
import { BaseEntity } from '../common/base.entity';

@Entity('apks')
@Index(['versionCode'])
export class Apk extends BaseEntity {
  @Column({ type: 'integer' })
  versionCode!: number;

  @Column({ type: 'varchar', length: 100, nullable: true })
  versionName!: string | null;

  @Column({ type: 'varchar', length: 200, nullable: true })
  applicationId!: string | null;

  /** Path relative to the assets public root, e.g. `apks/1699-abc.apk`. */
  @Column({ type: 'varchar', length: 500 })
  filePath!: string;

  @Column({ type: 'integer', default: 0 })
  sizeBytes!: number;
}
