# 브로트베르크 매장 월(wall) 메뉴판 시드

브로트베르크 매장의 가로 3연결 디스플레이(월) 구성 시드.

- `menu.json` — 사장님 제공 메뉴(빵 16종 / 음료 12종, 가격 포함). 단일 출처.
- `seed_boards.py` — **BAKERY / BEVERAGE** 메뉴판을 만든다. 브랜드명은 넣지
  않으며(월에서 한 번만 노출하기 위함), 각 패널은 여러 배경 사진을
  **크로스페이드 로테이션**한다.

## 월 구성 원칙

- display-1 = 제품 풀스크린 로테이션, display-2 = BAKERY, display-3 = BEVERAGE.
- 세 디스플레이는 **서로 겹치지 않는(disjoint) 이미지 풀**을 쓴다 → 어느 순간에도
  같은 사진이 두 화면에 동시에 뜨지 않는다. (display-1 제품 / display-2 베이커리
  배경 / display-3 음료 배경을 모두 다른 에셋으로 구성)
- 메뉴 패널에는 브랜드명을 넣지 않는다(섹션 제목 BAKERY / BEVERAGE만).

## 사용법

배경 이미지들을 먼저 어드민(에셋)에 업로드하고 그 asset id 목록을 넘긴다.

```bash
python3 seed_boards.py \
  --api http://display-1:4200/api \
  --bakery-bgs 28,29,30   --bakery-monitor 3 \
  --beverage-bgs 31,32,33 --beverage-monitor 4 \
  --interval-ms 9000 --fade-ms 900
```

각 배경마다 같은 메뉴의 화면을 1개씩 만들고, 해당 모니터를 그 화면들로
로테이션 설정한다. 메뉴·가격을 바꾸려면 `menu.json`만 고치고 다시 실행한다.

> display-1(제품 로테이션)은 이 스크립트 범위가 아니다. 제품 이미지를 풀스크린
> 화면으로 만들어 해당 모니터에 로테이션으로 할당하면 된다.
