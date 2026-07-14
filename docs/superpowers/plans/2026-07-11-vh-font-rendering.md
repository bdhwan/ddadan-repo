# Viewport-Relative Font Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Store text sizes as `vh` values in live screen layouts and render them proportionally on 1080p and 4K players.

**Architecture:** Add an optional `fontUnit` field to screen items and centralize CSS font-size generation in the player. Existing layouts without a unit remain `px`; migrated live menu layouts use `vh` values computed from their 1080px design height.

**Tech Stack:** Angular 21, TypeScript, Vitest, SQLite JSON layouts, Docker Compose

## Global Constraints

- Preserve existing positioning, images, colors, weights, rotation, and menu content.
- Convert every text item in screens 41–46 using `vh = px / 1080 * 100`.
- Capture the actual 3840×2160 Wayland outputs after deployment.

---

### Task 1: Player font unit support

**Files:**
- Modify: `apps/ddadan-client-app/src/app/app.ts`
- Modify: `apps/ddadan-client-app/src/app/app.html`
- Test: `apps/ddadan-client-app/src/app/app.spec.ts`

- [ ] Add a failing test proving `fontCss()` returns `3.7037037037vh` for a `40` value with `fontUnit: 'vh'` and `40px` for legacy data.
- [ ] Run the focused test and confirm it fails because `fontCss()` and `fontUnit` do not exist.
- [ ] Add `fontUnit?: 'px' | 'vh'` and the minimal `fontCss()` formatter.
- [ ] Bind all three player item render paths to `[style.font-size]="fontCss(item)"`.
- [ ] Run unit tests and production build.

### Task 2: Live layout migration and deployment

**Files:**
- Modify live SQLite rows: screens `41`–`46`
- Deploy: `ddadan-client-app` service on `display-1`

- [ ] Back up the live SQLite database.
- [ ] Convert every text item's `fontSize` from the 1080px design value to `vh` and set `fontUnit: 'vh'` in one transaction.
- [ ] Verify all text items have `fontUnit: 'vh'` and expected converted values.
- [ ] Rebuild and restart the player service, then confirm the health endpoint and player API respond.

### Task 3: Physical display verification

**Files:**
- Create: `artifacts/display-1-actual-monitor-vh-4k.png`
- Create: `artifacts/display-2-actual-monitor-vh-4k.png`

- [ ] Reload the kiosk browsers so they receive the new client bundle and layouts.
- [ ] Capture each Raspberry Pi Wayland output with `grim`.
- [ ] Verify both captures are 3840×2160 and visually inspect text wrapping and clipping.
- [ ] Present both physical-output captures to the user.
