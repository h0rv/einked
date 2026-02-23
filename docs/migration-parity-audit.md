# Einked Migration Parity Audit

This tracks behavioral/UI parity between pre-migration `xteink-ui` and current `einked-ereader`.

## Scope
- Baseline: pre-removal `xteink-ui` (around `6556985^`).
- Target: `einked` + `einked-ereader` only, no firmware coupling.

## Non-Negotiables
- EPUB rendering/layout uses `epub-stream` + `epub-stream-render`.
- No synthetic/dummy fallback data paths for feeds/epub content.
- Device specifics stay outside `einked`/`einked-ereader`.

## Parity Matrix
| Area | Before (xteink-ui) | Current (einked-ereader) | Status |
|---|---|---|---|
| EPUB paging | Chapter/page model with page-turn UX | Chapter/page modal state, explicit page/chapter counters, left/right page turns | In Progress |
| EPUB chapter jump | Volume buttons jump chapters | `Aux1/Aux2` jump chapter in EPUB modal | In Progress |
| EPUB footer info | Progress/footer controls shown while reading | Footer now shows `ch X/Y p A/B`; tab dots hidden in reader modal | In Progress |
| Feed data | Real OPDS/RSS integration | Real feed parse path; no synthetic entry lists | In Progress |
| File browser | Rich browser/task model | Flat list model currently | Gap |
| Reader settings | Full settings model affecting renderer | Static settings list currently | Gap |
| UI wrapping | Wrapped text in reader-focused surfaces | Runtime `paragraph` and `draw_text_at` wrapping implemented + regression tests | In Progress |

## Open Gaps (Must Fix)
1. Restore full reader settings surface and bind to render config.
2. Restore richer file-browser workflow parity (directory/task flow).
3. Complete EPUB reader parity polish (overlay/menu/progress behavior from old UX).

## Recent Changes
- Removed dummy fallback feed entries.
- Removed sample-book injection fallback in file list.
- Hardened canonical EPUB render path (no chapter-text bridge path).
- Added paged EPUB modal navigation:
  - Left/Right: page turns.
  - Aux1/Aux2: chapter jump.
- Added runtime wrapping for both `paragraph` and `draw_text_at`.

## Verification Checklist
- [ ] Open EPUB and turn pages with Left/Right.
- [ ] Jump chapters with Aux1/Aux2.
- [ ] Footer shows chapter/page counts with no tab dots overlay.
- [ ] Feed shows real parsed entries (no synthetic placeholders).
- [ ] Long labels/paragraphs wrap without truncation/overflow.
