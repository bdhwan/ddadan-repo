#!/usr/bin/env python3
"""Seed the Brotwerk store-wall menu boards — bright cafe style (rich menu cells).

각 품목 행을 단일 `menuLine` 아이템으로 저작한다. 한 아이템 안에서
  [원형 뱃지] 한글명  영문명 ····· 기본가 (+보조가)  [ICED Only 태그]
가 flex로 스스로 배치된다(웹 렌더러 `ddadan-client-app`가 렌더). 카테고리는
`groupHeader` 아이템(한글 + 영문 + 룰 라인 + 우측 라벨)으로 구분한다.

Layout (per panel, 1920x1080):
  - warm-white 메뉴 패널 + 바깥쪽 가장자리 풀높이 제품사진 스트립
    (BAKERY → 사진 좌측, BEVERAGE → 사진 우측)
  - BAKERY: 그룹 없이 2열 × 8행
  - BEVERAGE: 좌열 '커피/COFFEE'(EXTRA SIZE), 우열 '티·에이드/TEA & ADE'

각 패널은 여러 제품사진을 로테이션하며, 세 디스플레이는 서로 겹치지 않는
이미지 풀을 쓴다.

Usage:
    python3 seed_boards.py --api http://display-1:4200/api \
        --bakery-imgs 48,49,50   --bakery-monitor 3 \
        --beverage-imgs 51,52,53 --beverage-monitor 4
"""
import argparse, json, os, uuid, urllib.request

# Bright, colorful franchise-board palette
BG = "#fdfcf9"; TITLE = "#21395f"; KO = "#5a7aa6"; NAME = "#2f2a24"
PRICE = "#e0552f"; ROWL = "rgba(33,57,95,0.09)"; DIV = "rgba(33,57,95,0.18)"
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


def menuline(m, x, y, w, h, z, size):
    """단일 메뉴 행: 뱃지+한글+영문 ····· 가격(+보조가). price는 숫자(원 포맷) 또는 문자열."""
    price = m.get("price")
    sec = won(price) if isinstance(price, (int, float)) else (price or "")
    it = {"id": str(uuid.uuid4()), "kind": "text", "textVariant": "menuLine",
          "text": m["name"], "textSecondary": sec, "priceColor": PRICE,
          "x": x, "y": y, "width": w, "height": h, "zIndex": z,
          "fontSize": size, "color": NAME, "fontWeight": 600}
    if m.get("en"): it["textEn"] = m["en"]
    if m.get("extra"): it["priceExtra"] = m["extra"]
    if m.get("badges"): it["badges"] = m["badges"]
    return it


def groupheader(ko, en, right, x, y, w, h, z, size, color=TITLE):
    it = {"id": str(uuid.uuid4()), "kind": "text", "textVariant": "groupHeader",
          "text": ko, "x": x, "y": y, "width": w, "height": h, "zIndex": z,
          "fontSize": size, "color": color, "fontWeight": 800}
    if en: it["textEn"] = en
    if right: it["textSecondary"] = right
    return it


def note(text, tint, color, x, y, w, h, z, size):
    """안내 콜백 박스(라운드 틴트 + 체크 + 문구)."""
    return {"id": str(uuid.uuid4()), "kind": "text", "textVariant": "note", "text": text,
            "background": tint, "color": color, "fontSize": size, "fontWeight": 700,
            "radius": 16, "x": x, "y": y, "width": w, "height": h, "zIndex": z}


def card(x, y, w, h, z, bg, radius=22):
    """라운드 배경 카드/패널."""
    it = box(x, y, w, h, z, bg)
    it["radius"] = radius
    return it


# 음료 그룹: (한글, 영문, 헤더색, 안내박스 틴트, 그룹카드 틴트)
# 박스 디스플레이는 저알파를 워시아웃하므로 카드 틴트를 충분히 진하게(그룹 구분 확실).
BEV = {
    "espresso": ("에스프레소", "ESPRESSO", "#2f6fd0", "rgba(47,111,208,0.30)", "rgba(47,111,208,0.16)"),
    "coldbrew": ("콜드브루 · 에이드", "COLD BREW · ADE", "#1a9b90", "rgba(26,155,144,0.30)", "rgba(26,155,144,0.17)"),
    "tea": ("티 · 주스", "TEA · JUICE", "#2f9e6e", "rgba(47,158,110,0.30)", "rgba(47,158,110,0.16)"),
}

# 베이커리 그룹: (한글, 영문, 헤더색, 안내박스 틴트, 그룹카드 틴트) — 웜톤
BAKERY = {
    "bread": ("빵 · 베이커리", "BREAD", "#b5731f", "rgba(181,115,31,0.30)", "rgba(181,115,31,0.15)"),
    "sweet": ("단과자 · 잼", "PASTRY · JAM", "#c0392b", "rgba(192,57,43,0.28)", "rgba(192,57,43,0.13)"),
    "sandwich": ("샌드위치", "SANDWICH", "#4a8c5f", "rgba(74,140,95,0.30)", "rgba(74,140,95,0.15)"),
}


def _kprice(p):
    """가격을 천 단위 소수 표기로: 4000->'4.0', 3500->'3.5'. 문자열이면 +extra로 처리."""
    if isinstance(p, (int, float)):
        return f"{p / 1000:.1f}"
    return _kextra(p)


def _kextra(e):
    """'+1,000'->'+1.0', '+500'->'+0.5'."""
    if not e:
        return e
    num = int(str(e).replace("+", "").replace(",", "").strip())
    return f"+{num / 1000:.1f}"


def beverage_layout(title_en, title_ko, groups, addons, img_asset, img_w=481):
    """리치 음료 보드: 색상 그룹 카드 3개 + 안내박스 + 추가 카드 (사진 우측, 큰 폰트).

    가격은 천 단위 소수 표기(4,000->4.0, +1,000->+1.0)로 짧게 표시해 공간을 절약한다.
    """
    # 가격/보조가를 천 단위 소수 문자열로 변환(원본 메뉴 데이터는 그대로).
    groups = {
        k: [{**m, "price": _kprice(m["price"]),
             **({"extra": _kextra(m["extra"])} if m.get("extra") else {})} for m in v]
        for k, v in groups.items()
    }
    addons = [{**a, "price": _kextra(a["price"])} for a in addons]

    it = [box(0, 0, W, H, 0, BG)]
    img_x = W - img_w
    it.append(img(img_asset, img_x, 0, img_w, H, 1))

    menu_l = 28
    menu_r = img_x - 32
    menu_w = menu_r - menu_l
    col_gap = 40
    col_w = (menu_w - col_gap) // 2
    xs = [menu_l, menu_l + col_w + col_gap]

    # 보드 타이틀(BEVERAGE/음료) 없음 — 그 공간까지 그룹 카드로 사용.
    item_size = 34
    row_h = 66
    hdr_size = 38
    hdr_h = 48
    px = 32           # 카드 내부 좌우 여백
    pad_top = 32      # 카드 상단 여백
    pad_bot = 30      # 카드 하단 여백
    hdr_gap = 20      # 헤더 밑줄~첫 항목 간격
    gap = 30          # 카드 사이 간격
    top = 28

    def place_card(key, x, y, rh):
        ko, en, color, ntint, ctint = BEV[key]
        rows = groups[key]
        # 그룹의 다수(2개 이상)가 사이즈업일 때만 헤더에 EXTRA SIZE 라벨(좁은 카드 충돌 방지).
        right = "EXTRA SIZE" if sum(1 for m in rows if m.get("extra")) >= 2 else None
        ch = pad_top + hdr_h + hdr_gap + len(rows) * rh + pad_bot
        it.append(card(x, y, col_w, ch, 4, ctint, 24))
        it.append(card(x + 24, y + pad_top + 2, 9, hdr_h - 10, 6, color, 5))   # 컬러 액센트 바
        it.append(groupheader(ko, en, right, x + 48, y + pad_top, col_w - 72, hdr_h, 6, hdr_size, color))
        it.append(box(x + px, y + pad_top + hdr_h + 4, col_w - 2 * px, 2, 6, "rgba(33,57,95,0.12)"))
        iy = y + pad_top + hdr_h + hdr_gap
        for m in rows:
            it.append(menuline(m, x + px, iy, col_w - 2 * px, rh, 6, item_size))
            it.append(box(x + px, iy + rh - 8, col_w - 2 * px, 1, 6, ROWL))
            iy += rh
        return y + ch

    # 좌열: 에스프레소 카드 + 안내박스 + 추가 카드(하단 정렬)
    y1 = place_card("espresso", xs[0], top, row_h)
    it.append(note("모든 커피는 최상급 원두를 사용합니다", BEV["espresso"][3],
                   BEV["espresso"][2], xs[0], y1 + gap, col_w, 72, 6, int(item_size * 0.8)))
    # 추가 카드 — 하단에 붙여 세로 여백 채움
    nrows = (len(addons) + 1) // 2
    add_h = pad_top + hdr_h + hdr_gap + nrows * 62 + pad_bot
    add_y = H - 44 - add_h
    it.append(card(xs[0], add_y, col_w, add_h, 4, "rgba(60,46,34,0.10)", 24))
    it.append(card(xs[0] + 24, add_y + pad_top + 2, 9, hdr_h - 10, 6, NAME, 5))
    it.append(groupheader("추가", "Add", None, xs[0] + 48, add_y + pad_top, col_w - 72, hdr_h, 6, int(item_size * 0.9), NAME))
    it.append(box(xs[0] + px, add_y + pad_top + hdr_h + 4, col_w - 2 * px, 2, 6, "rgba(33,57,95,0.10)"))
    ay = add_y + pad_top + hdr_h + hdr_gap
    half = (col_w - 2 * px) // 2
    for i, a in enumerate(addons):
        ax = xs[0] + px + (i % 2) * half
        ry = ay + (i // 2) * 62
        it.append(menuline({"name": a["name"], "en": a["en"], "price": a["price"]},
                           ax, ry, half - 14, 56, 7, int(item_size * 0.82)))

    # 우열: 콜드브루 카드 + 티·주스 카드
    y2 = place_card("coldbrew", xs[1], top, row_h)
    place_card("tea", xs[1], y2 + gap, row_h)

    return {"width": W, "height": H, "layout": {"items": it}}


def bakery_layout(groups, note_text, img_asset, img_w=481):
    """리치 베이커리 보드: 색상 그룹 카드(빵/단과자·잼/샌드위치) + 안내박스 (사진 좌측, 큰 폰트).
    음료 보드와 동일한 카드 스타일. 가격은 천 단위 소수 표기."""
    groups = {
        k: [{**m, "price": _kprice(m["price"]),
             **({"extra": _kextra(m["extra"])} if m.get("extra") else {})} for m in v]
        for k, v in groups.items()
    }
    it = [box(0, 0, W, H, 0, BG)]
    it.append(img(img_asset, 0, 0, img_w, H, 1))          # 사진 좌측

    menu_l = img_w + 36
    menu_r = W - 28
    menu_w = menu_r - menu_l
    col_gap = 40
    col_w = (menu_w - col_gap) // 2
    xs = [menu_l, menu_l + col_w + col_gap]

    item_size = 34
    row_h = 80
    hdr_size = 38
    hdr_h = 48
    px = 32
    pad_top = 32
    pad_bot = 30
    hdr_gap = 20
    gap = 30
    top = 28

    def place_card(key, x, y, rh):
        ko, en, color, ntint, ctint = BAKERY[key]
        rows = groups[key]
        right = "EXTRA SIZE" if sum(1 for m in rows if m.get("extra")) >= 2 else None
        ch = pad_top + hdr_h + hdr_gap + len(rows) * rh + pad_bot
        it.append(card(x, y, col_w, ch, 4, ctint, 24))
        it.append(card(x + 24, y + pad_top + 2, 9, hdr_h - 10, 6, color, 5))
        it.append(groupheader(ko, en, right, x + 48, y + pad_top, col_w - 72, hdr_h, 6, hdr_size, color))
        it.append(box(x + px, y + pad_top + hdr_h + 4, col_w - 2 * px, 2, 6, "rgba(33,57,95,0.12)"))
        iy = y + pad_top + hdr_h + hdr_gap
        for m in rows:
            it.append(menuline(m, x + px, iy, col_w - 2 * px, rh, 6, item_size))
            it.append(box(x + px, iy + rh - 8, col_w - 2 * px, 1, 6, ROWL))
            iy += rh
        return y + ch

    # 좌열(메뉴): 빵 카드 + 안내박스 / 우열: 단과자 카드 + 샌드위치 카드
    y1 = place_card("bread", xs[0], top, row_h)
    it.append(note(note_text, BAKERY["bread"][3], BAKERY["bread"][2],
                   xs[0], y1 + gap, col_w, 72, 6, int(item_size * 0.8)))
    y2 = place_card("sweet", xs[1], top, row_h)
    place_card("sandwich", xs[1], y2 + gap, row_h)

    return {"width": W, "height": H, "layout": {"items": it}}


def panel_layout(side, title_en, title_ko, columns, top, line_h, size, img_asset):
    """columns: [{"header": (ko, en, right) | None, "rows": [menu items]}, ...] (최대 2열)."""
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
    row_h = int(size * 1.7)
    hdr_h = int(size * 2.0)

    for ci, col in enumerate(columns):
        x = xs[ci]
        y = top
        header = col.get("header")
        if header:
            ko, en, right = header
            it.append(groupheader(ko, en, right, x, y, col_w, hdr_h, 6, int(size * 0.92)))
            it.append(box(x, y + hdr_h - 6, col_w, 2, 6, DIV))
            y += hdr_h + int(size * 0.5)
        for m in col["rows"]:
            it.append(menuline(m, x, y, col_w, row_h, 6, size))
            it.append(box(x, y + row_h - 6, col_w, 1, 6, ROWL))
            y += row_h
    return {"width": W, "height": H, "layout": {"items": it}}


def seed_panel(base, side, title_en, title_ko, columns, top, line_h, size,
               img_assets, monitor, interval_ms, fade_ms):
    ids = []
    for i, a in enumerate(img_assets, 1):
        payload = {"name": f"{title_en} {i}",
                   **panel_layout(side, title_en, title_ko, columns, top, line_h, size, a)}
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

    bread = menu["bread"]
    bakery_cols = [
        {"header": None, "rows": bread[:8]},
        {"header": None, "rows": bread[8:16]},
    ]
    print("BAKERY panel (photo left):")
    seed_panel(args.api, "left", "BAKERY", "베이커리", bakery_cols, 248, 78, 30,
               args.bakery_imgs, args.bakery_monitor, args.interval_ms, args.fade_ms)

    bev = menu["beverage"]
    groups = {k: [m for m in bev if m.get("group") == k] for k in ("espresso", "coldbrew", "tea")}
    addons = menu.get("beverage_addons", [])
    print("BEVERAGE panel (rich: 3 groups + note + add):")
    ids = []
    for i, a in enumerate(args.beverage_imgs, 1):
        payload = {"name": f"BEVERAGE {i}", **beverage_layout("BEVERAGE", "음료", groups, addons, a)}
        sid = api(args.api, "POST", "/screens", payload)["id"]
        ids.append(sid)
        print(f"  BEVERAGE img#{i} (asset {a}) -> screen {sid}")
    api(args.api, "PATCH", f"/monitors/{args.beverage_monitor}/rotation",
        {"screenIds": ids, "intervalMs": args.interval_ms, "fadeMs": args.fade_ms})
    print(f"  monitor {args.beverage_monitor} rotation -> {ids}")
    print("done.")


if __name__ == "__main__":
    main()
