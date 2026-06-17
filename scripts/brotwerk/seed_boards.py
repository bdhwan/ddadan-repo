#!/usr/bin/env python3
"""Seed the Brotwerk store-wall menu boards (BAKERY + BEVERAGE).

The three displays form ONE continuous wall, so:
  - no brand name on the menu panels (just the section title BAKERY / BEVERAGE),
  - each menu panel rotates through several background photos, and
  - every display draws from a DISJOINT image pool, so no two screens ever
    show the same picture at the same time.

For each panel this builds one screen per background image (same menu, different
backdrop) and sets the monitor to crossfade-rotate through them.

Usage:
    python3 seed_boards.py --api http://display-1:4200/api \
        --bakery-bgs 28,29,30   --bakery-monitor 3 \
        --beverage-bgs 31,32,33 --beverage-monitor 4

Background asset ids must already be uploaded (admin → assets). Run from this
directory (reads ./menu.json).
"""
import argparse, json, os, uuid, urllib.request

WHITE = "#f7f4ee"; GOLD = "#e8c98c"; CREAM = "#e9e3d6"; SUB = "#9aa6c0"
DIM = "rgba(7,11,24,0.76)"; LINE = "rgba(232,201,140,0.5)"; ROW = "rgba(255,255,255,0.08)"


def api(base, method, path, body=None):
    req = urllib.request.Request(
        base + path,
        data=json.dumps(body).encode() if body is not None else None,
        method=method, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def img(a, x, y, w, h, z):
    return {"id": str(uuid.uuid4()), "kind": "image", "assetId": a,
            "x": x, "y": y, "width": w, "height": h, "zIndex": z}

def txt(s, x, y, w, h, z, size, color, weight=None, align=None):
    it = {"id": str(uuid.uuid4()), "kind": "text", "text": s, "x": x, "y": y,
          "width": w, "height": h, "zIndex": z, "fontSize": size, "color": color}
    if weight: it["fontWeight"] = weight
    if align: it["textAlign"] = align
    return it

def box(x, y, w, h, z, bg):
    return {"id": str(uuid.uuid4()), "kind": "text", "text": "",
            "x": x, "y": y, "width": w, "height": h, "zIndex": z, "background": bg}

def won(n):
    return f"{n:,}"


def header(it, title_en, title_ko):
    it.append(txt(title_en, 0, 104, 1920, 80, 5, 66, WHITE, 800, "center"))
    it.append(txt(title_ko, 0, 198, 1920, 38, 5, 26, GOLD, 600, "center"))
    it.append(box(835, 256, 250, 2, 5, LINE))


def add_rows(it, items, per_col, top, line_h, size):
    col_w, gap = 700, 120
    left = (1920 - (col_w * 2 + gap)) // 2
    xs = [left, left + col_w + gap]
    name_w, price_off = 440, 470
    for i, m in enumerate(items):
        c = min(i // per_col, 1)
        x, y = xs[c], top + (i % per_col) * line_h
        it.append(txt(m["name"], x, y, name_w, line_h - 12, 6, size, CREAM, 500))
        it.append(txt(won(m["price"]), x + price_off, y, col_w - price_off, line_h - 12, 6, size, GOLD, 700, "right"))
        it.append(box(x, y + line_h - 8, col_w, 1, 6, ROW))


def build_layout(bg_asset, title_en, title_ko, items, per_col, top, line_h, size, footer):
    it = [img(bg_asset, 0, 0, 1920, 1080, 0), box(0, 0, 1920, 1080, 1, DIM)]
    header(it, title_en, title_ko)
    add_rows(it, items, per_col, top, line_h, size)
    it.append(txt(footer, 0, 1004, 1920, 40, 6, 22, SUB, 400, "center"))
    return {"width": 1920, "height": 1080, "layout": {"items": it}}


def seed_panel(base, name, title_en, title_ko, items, per_col, top, line_h, size,
               footer, bg_assets, monitor, interval_ms, fade_ms):
    screen_ids = []
    for i, bg in enumerate(bg_assets, 1):
        payload = {"name": f"{title_en} {i}", **build_layout(bg, title_en, title_ko, items, per_col, top, line_h, size, footer)}
        sid = api(base, "POST", "/screens", payload)["id"]
        screen_ids.append(sid)
        print(f"  {title_en} bg#{i} (asset {bg}) -> screen {sid}")
    api(base, "PATCH", f"/monitors/{monitor}/rotation",
        {"screenIds": screen_ids, "intervalMs": interval_ms, "fadeMs": fade_ms})
    print(f"  monitor {monitor} rotation -> {screen_ids} ({interval_ms}ms)")
    return screen_ids


def csv_ints(s):
    return [int(x) for x in s.split(",") if x.strip()]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--api", default="http://display-1:4200/api")
    ap.add_argument("--bakery-bgs", type=csv_ints, required=True)
    ap.add_argument("--bakery-monitor", type=int, required=True)
    ap.add_argument("--beverage-bgs", type=csv_ints, required=True)
    ap.add_argument("--beverage-monitor", type=int, required=True)
    ap.add_argument("--interval-ms", type=int, default=9000)
    ap.add_argument("--fade-ms", type=int, default=900)
    args = ap.parse_args()

    menu = json.load(open(os.path.join(os.path.dirname(__file__), "menu.json")))
    print("BAKERY panel:")
    seed_panel(args.api, "bakery", "BAKERY", "베이커리", menu["bread"], 8, 344, 80, 35,
               "매장에서 갓 구워낸 빵 · 매일 오전 11시 출고",
               args.bakery_bgs, args.bakery_monitor, args.interval_ms, args.fade_ms)
    print("BEVERAGE panel:")
    seed_panel(args.api, "beverage", "BEVERAGE", "음료", menu["beverage"], 6, 372, 98, 37,
               "핸드드립 커피 · 생과일 주스는 매일 신선하게",
               args.beverage_bgs, args.beverage_monitor, args.interval_ms, args.fade_ms)
    print("done.")


if __name__ == "__main__":
    main()
