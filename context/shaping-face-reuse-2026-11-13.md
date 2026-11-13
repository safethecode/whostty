# Shaping font face reuse — 2026-11-13

Goal: avoid reparsing the same font for every terminal cell during a layout-cache miss.

Design: each Renderer::shape call owns a lazy cache of borrowed rustybuzz faces. A font is parsed at most once for that call, including failed parses. Unused fallback fonts remain unparsed. The cache is dropped after the call so it does not retain new global font data or need self-referencing lifetimes. Font selection order, script boundaries, GSUB/GPOS, glyph metrics, RTL/color fallback and existing layout/atlas limits stay the same.

Alternatives: parsing each probe repeats table work; persistent parsed faces require ownership/lifetime machinery beyond this change. Use a per-call OnceCell cache with references into the renderer's existing Arc<FontData> sources.

Implementation and validation:

- [ ] Add a failing regression for lazy valid/invalid font parsing and reference reuse; preserve all existing shaping output tests.
- [ ] Implement the local cache and route both font selection and shaping through it.
- [ ] Run Windows/macOS tests, fmt/Clippy, Python suite, Windows native package and memory ceilings.
- [ ] Download baseline/current packages and measure the same RTX3080Ti native output workload. Record exact hashes, CPU and memory with limits; do not infer physical display latency.
- [ ] Review, commit, PR, merge after green checks, verify main, create the next clean branch.

Latest starting point is main86d7ce9, including PR26/27 memory optimizations. Earlier500MiB values describe an older binary. Source and hardware measurements must be distinguished. No Windows reboot is authorized. WSL remains uninstalled after the previous elevation cancellation.
