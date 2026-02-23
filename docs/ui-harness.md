# einked UI Harness

`einked-ui-harness` is a deterministic host-side regression harness for `einked-ereader`.

## Scope

Current scenarios cover:

- library list rendering and row wrapping guardrails
- EPUB open and render path (real fixture EPUB, no dummy text)
- EPUB navigation (`Right` page turn, `Aux2` chapter jump) and footer metrics
- settings tab navigation and value mutation via `Confirm`
- feed modal flow (`Feed` -> entries -> item -> back navigation)

## Run

From `einked/`:

```bash
just ui-audit
```

or directly:

```bash
RUSTC_WRAPPER= cargo test --manifest-path crates/einked-ui-harness/Cargo.toml --test ui_regression -- --nocapture
```

## Artifacts

Each scenario writes text + PNG captures under:

- `einked/crates/einked-ui-harness/target/ui-audit/library_epub/`
- `einked/crates/einked-ui-harness/target/ui-audit/settings_txt/`
- `einked/crates/einked-ui-harness/target/ui-audit/feeds_flow/`

Each `step-XX.txt` stores extracted draw text with coordinates, and `step-XX.png` stores a rasterized frame for quick visual diffing.
