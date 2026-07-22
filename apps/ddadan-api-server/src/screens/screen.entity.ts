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
  /**
   * 'menuLine' renders label + dot leader + price (textSecondary).
   * 'groupHeader' renders a category header (한글 title + textEn + right-aligned textSecondary).
   * 'note' renders a tinted rounded callout box with a leading check + text.
   */
  textVariant?: 'plain' | 'menuLine' | 'groupHeader' | 'note';
  textSecondary?: string;
  /** 영문 병기(한글명 옆 작은 회색 텍스트). */
  textEn?: string;
  /** 이중 가격의 보조가("+1.0" 등) — textSecondary(기본가) 뒤에 강조 표시. */
  priceExtra?: string;
  /** menuLine 가격 색(미지정 시 아이템 색 상속). */
  priceColor?: string;
  /** 인라인 뱃지(BEST/추천/ICED Only/DECAF 등). */
  badges?: { text: string; variant?: 'best' | 'rec' | 'info' | 'warn' }[];
  x: number;
  y: number;
  width: number;
  height: number;
  zIndex?: number;
  /** 배경 모서리 둥글기(px) — 그룹 카드/패널/안내박스. */
  radius?: number;
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
