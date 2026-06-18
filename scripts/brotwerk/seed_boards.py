#!/usr/bin/env python3
"""Seed the Brotwerk store-wall menu boards — bright cafe style.

Layout (per panel, 1920x1080), inspired by a typical bright franchise menu wall:
  - warm-white menu panel with dark text and accent prices, and
  - a full-height product-photo strip on the OUTER edge of the wall
    (BAKERY on display-2 → photo on the left; BEVERAGE on display-3 → photo
    on the right), so the wall is framed by appetizing product shots.

Each panel rotates through several product photos (same menu, different photo),
and the three displays draw from DISJOINT image pools so no two screens ever
show the same picture at once. No brand name on the menu panels (shown once
elsewhere on the wall) — just the section title BAKERY / BEVERAGE.

Usage:
    python3 seed_boards.py --api http://display-1:4200/api \
        --bakery-imgs 48,49,50   --bakery-monitor 3 \
        --beverage-imgs 51,52,53 --beverage-monitor 4
"""
import argparse, json, os, uuid, urllib.request

BG = "#f6f2ea"; TITLE = "#3a2e22"; KO = "#b0894a"; NAME = "#4a3f33"
PRICE = "#b07a2e"; ROWL = "rgba(60,46,34,0.10)"; FOOT = "#9a8e7c"; DIV = "rgba(60,46,34,0.16)"
W, H = 1920, 1080
IMG_W = 620


def api(base, method, path, body=None):
    req = urllib.request.Request(
        base + path, data=json.dumps(body).encode() if body is not None else None,
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


def panel_layout(side, title_en, title_ko, items, per_col, top, line_h, size, footer, img_asset):
    it = [box(0, 0, W, H, 0, BG)]
    if side == "left":
        img_x, div_x, menu_l = 0, IMG_W, IMG_W + 90
    else:
        img_x, div_x, menu_l = W - IMG_W, W - IMG_W - 3, 90
    it.append(img(img_asset, img_x, 0, IMG_W, H, 1))
    it.append(box(div_x, 0, 3, H, 2, "rgba(60,46,34,0.12)"))
    menu_r = (W - 90) if side == "left" else (W - IMG_W - 90)
    menu_w = menu_r - menu_l

    it.append(txt(title_en, menu_l, 72, menu_w, 80, 5, 60, TITLE, 800, "left"))
    it.append(txt(title_ko, menu_l, 154, menu_w, 36, 5, 24, KO, 600, "left"))
    it.append(box(menu_l, 206, int(menu_w * 0.5), 2, 5, DIV))

    col_gap = 70
    col_w = (menu_w - col_gap) // 2
    xs = [menu_l, menu_l + col_w + col_gap]
    name_w = col_w - 130
    for i, m in enumerate(items):
        c = min(i // per_col, 1)
        x, y = xs[c], top + (i % per_col) * line_h
        it.append(txt(m["name"], x, y, name_w, line_h - 14, 6, size, NAME, 500))
        it.append(txt(won(m["price"]), x + col_w - 150, y, 150, line_h - 14, 6, size, PRICE, 700, "right"))
        it.append(box(x, y + line_h - 10, col_w, 1, 6, ROWL))

    it.append(txt(footer, menu_l, 996, menu_w, 36, 6, 21, FOOT, 400, "left"))
    return {"width": W, "height": H, "layout": {"items": it}}


def seed_panel(base, side, title_en, title_ko, items, per_col, top, line_h, size,
               footer, img_assets, monitor, interval_ms, fade_ms):
    ids = []
    for i, a in enumerate(img_assets, 1):
        payload = {"name": f"{title_en} {i}",
                   **panel_layout(side, title_en, title_ko, items, per_col, top, line_h, size, footer, a)}
        sid = api(base, "POST", "/screens", payload)["id"]
        ids.append(sid)
        print(f"  {title_en} img#{i} (asset {a}) -> screen {sid}")
    api(base, "PATCH", f"/monitors/{monitor}/rotation",
        {"screenIds": ids, "intervalMs": interval_ms, "fadeMs": fade_ms})
    print(f"  monitor {monitor} rotation -> {ids}")
    return ids


def csv_ints(s):
    return [int(x) for x in s.split(",") if x.strip()]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--api", default="http://display-1:4200/api")
    ap.add_argument("--bakery-imgs", type=csv_ints, required=True)
    ap.add_argument("--bakery-monitor", type=int, required=True)
    ap.add_argument("--beverage-imgs", type=csv_ints, required=True)
    ap.add_argument("--beverage-monitor", type=int, required=True)
    ap.add_argument("--interval-ms", type=int, default=9000)
    ap.add_argument("--fade-ms", type=int, default=900)
    args = ap.parse_args()

    menu = json.load(open(os.path.join(os.path.dirname(__file__), "menu.json")))
    print("BAKERY panel (photo left):")
    seed_panel(args.api, "left", "BAKERY", "베이커리", menu["bread"], 8, 248, 78, 30,
               "매장에서 갓 구워낸 빵 · 매일 오전 11시 출고",
               args.bakery_imgs, args.bakery_monitor, args.interval_ms, args.fade_ms)
    print("BEVERAGE panel (photo right):")
    seed_panel(args.api, "right", "BEVERAGE", "음료", menu["beverage"], 6, 286, 96, 32,
               "핸드드립 커피 · 생과일 주스는 매일 신선하게",
               args.beverage_imgs, args.beverage_monitor, args.interval_ms, args.fade_ms)
    print("done.")


if __name__ == "__main__":
    main()
