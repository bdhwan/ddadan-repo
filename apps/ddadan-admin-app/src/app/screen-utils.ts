import { AssetView, ScreenLayoutItem, ScreenView } from './api.service';

const GRID = 8;

export function snapToGrid(value: number): number {
  return Math.round(value / GRID) * GRID;
}

export function screenThumbnailUrl(
  screen: ScreenView,
  assets: AssetView[],
  absoluteUrl: (path: string | null | undefined) => string,
): string | null {
  const img = screen.layout.items.find((i) => i.kind === 'image' && i.assetId);
  if (img?.assetId) {
    const asset = assets.find((a) => a.id === img.assetId);
    if (asset?.url) return absoluteUrl(asset.url);
  }
  return null;
}

export function isMenuLine(item: ScreenLayoutItem): boolean {
  return item.kind === 'text' && item.textVariant === 'menuLine';
}

export function textAlignStyle(item: ScreenLayoutItem): string {
  return item.textAlign ?? (isMenuLine(item) ? 'left' : 'center');
}
