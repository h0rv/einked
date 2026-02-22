# einked v1 implementation status

Last updated: 2026-02-22

## Required for firmware integration

- [x] Core primitives (`Rect`, `Point`, `Color`, `TextStyle`, `Theme`)
- [x] Generic input model (`InputEvent`, logical buttons, remapping helper)
- [x] Activity model (`Activity`, `Transition`, lifecycle hooks)
- [x] `alloc` + `no_alloc` stack paths (`Box` + static pool/factory)
- [x] Fixed-capacity layout builder
- [x] Render IR (`DrawCmd`, `CmdBuffer`, dirty tracker)
- [x] Refresh scheduler (`Adaptive/Full/Partial/Fast`, partial-limit ghosting guard)
- [x] End-to-end frame pipeline (`begin_frame` -> diff -> scheduler flush)
- [x] Generic component runtime path (`render_to_runtime` on shared components)
- [x] `ui!` proc macro baseline + refresh annotations
- [x] Simulators live in `einked` repo (desktop + web crates)
- [x] No direct firmware/device coupling in core types and traits

## High-priority DX surface (in progress)

- [~] `ui!` DSL coverage expanded (now: `VStack`, `HStack`, `Label`, `Paragraph`, `TextFlow`, `Icon`, `PageIndicator`, `Divider`, `StatusBar`, `Spacer`, refresh attrs)
- [ ] Add richer container options parity with spec examples (`align`, `fill` semantics)
- [ ] Add compile-fail tests for macro diagnostics and malformed trees

## Post-v1 / optional

- [ ] Split into `einked-core`, `einked-fonts`, `einked-epub` crates
- [ ] Font raster/cache crate (`GlyphRasterizer`, `fontdue` backend, configurable LRU)
- [ ] EPUB bridge crate on top of `epub-stream`
- [ ] Typed settings macro
- [ ] Advanced widget set parity with full spec examples
