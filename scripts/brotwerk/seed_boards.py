#!/usr/bin/env python3
"""Seed the Brotwerk (브로트베르크) bread & beverage menu boards.

Builds two 1920x1080 menu-board screens — a dimmed background photo with a
centered brand header and a two-column name/price menu (no product images) —
and PATCHes them onto existing screen ids (or creates them).

Usage:
    python3 seed_boards.py --api http://display-1:4200/api \
        --bg-asset 18 --bread-screen 12 --beverage-screen 13

`--bg-asset` is an uploaded image asset id used as the dimmed backdrop.
Omit `--bread-screen`/`--beverage-screen` to create new screens instead.
Run from this directory (reads ./menu.json).
"""
import argparse, json, os, uuid, urllib.request

WHITE = "#f7f4ee"; GOLD = "#e8c98c"; CREAM = "#e9e3d6"; SUB = "#9aa6c0"
DIM = "rgba(7,11,24,0.76)"; LINE = "rgba(232,201,140,0.5)"; ROW = "rgba(255,255,255,0.08)"


def api(base, method, path, body=None):
    req = urllib.request.Request(
        base + path,
        data=json.dumps(body).encode() if body is not None else None,
        method=method, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as r:
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


def base_layout(bg_asset, section):
    it = [img(bg_asset, 0, 0, 1920, 1080, 0), box(0, 0, 1920, 1080, 1, DIM)]
    it.append(txt("브로트베르크", 0, 96, 1920, 92, 5, 80, WHITE, 800, "center"))
    it.append(txt("B A K E R Y   ·   C O F F E E", 0, 196, 1920, 38, 5, 23, GOLD, 600, "center"))
    it.append(box(835, 252, 250, 2, 5, LINE))
    it.append(txt(section, 0, 272, 1920, 40, 5, 25, GOLD, 700, "center"))
    return it


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


def build_board(bg_asset, section, items, per_col, top, line_h, size, footer):
    it = base_layout(bg_asset, section)
    add_rows(it, items, per_col, top, line_h, size)
    it.append(txt(footer, 0, 1004, 1920, 40, 6, 22, SUB, 400, "center"))
    return {"width": 1920, "height": 1080, "layout": {"items": it}}


def upsert(base, screen_id, name, payload):
    if screen_id:
        api(base, "PATCH", f"/screens/{screen_id}", payload)
        print(f"  updated screen {screen_id}: {name}")
        return screen_id
    created = api(base, "POST", "/screens", {"name": name, **payload})
    print(f"  created screen {created['id']}: {name}")
    return created["id"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--api", default="http://display-1:4200/api")
    ap.add_argument("--bg-asset", type=int, required=True)
    ap.add_argument("--bread-screen", type=int)
    ap.add_argument("--beverage-screen", type=int)
    args = ap.parse_args()

    menu = json.load(open(os.path.join(os.path.dirname(__file__), "menu.json")))
    bread = build_board(args.bg_asset, "BREAD   ·   빵", menu["bread"], 8, 366, 80, 35,
                        "매장에서 갓 구워낸 빵 · 매일 오전 11시 출고")
    bev = build_board(args.bg_asset, "BEVERAGE   ·   음료", menu["beverage"], 6, 392, 98, 37,
                      "핸드드립 커피 · 생과일 주스는 매일 신선하게")
    upsert(args.api, args.bread_screen, "브로트베르크-빵메뉴판", bread)
    upsert(args.api, args.beverage_screen, "브로트베르크-음료메뉴판", bev)
    print("done.")


if __name__ == "__main__":
    main()
