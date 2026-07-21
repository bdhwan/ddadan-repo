#!/usr/bin/env python3
"""Remove the bottom footer caption(s) from already-seeded Brotwerk boards.

The footer text lives as a `text` layout item inside each rotating screen. This
scans all screens, drops any text item whose content matches a known caption,
and PATCHes the screen back with the full modified layout (the API replaces the
whole `layout`, there is no per-item patch).

Usage:
    python3 remove_footer.py --api http://display-1:4200/api
    python3 remove_footer.py --api http://100.96.152.109:7800/api   # display-4 direct
"""
import argparse, json, urllib.request

CAPTIONS = {
    "매장에서 갓 구워낸 빵 · 매일 오전 11시 출고",
    "핸드드립 커피 · 생과일 주스는 매일 신선하게",
}


def api(base, method, path, body=None):
    req = urllib.request.Request(
        base + path,
        data=json.dumps(body).encode() if body is not None else None,
        method=method,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--api", default="http://display-1:4200/api")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    screens = api(args.api, "GET", "/screens")
    changed = 0
    for s in screens:
        layout = s.get("layout") or {}
        items = layout.get("items") or []
        kept = [it for it in items
                if not (it.get("kind") == "text" and it.get("text") in CAPTIONS)]
        removed = len(items) - len(kept)
        if removed == 0:
            continue
        print(f"screen {s['id']} ({s.get('name')}): footer 아이템 {removed}개 제거")
        if args.dry_run:
            continue
        new_layout = {**layout, "items": kept}
        api(args.api, "PATCH", f"/screens/{s['id']}", {"layout": new_layout})
        changed += 1
    print(f"done. {changed}개 화면 업데이트{' (dry-run)' if args.dry_run else ''}.")


if __name__ == "__main__":
    main()
