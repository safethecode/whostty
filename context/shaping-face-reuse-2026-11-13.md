# Shaping font face reuse — 2026-11-13

Goal: avoid reparsing the same font for every terminal cell during a layout-cache miss.

Design: each Renderer::shape call owns a lazy cache of borrowed rustybuzz faces. A font is parsed at most once for that call, including failed parses. Unused fallback fonts remain unparsed. The cache is dropped after the call so it does not retain new global font data or need self-referencing lifetimes. Font selection order, script boundaries, GSUB/GPOS, glyph metrics, RTL/color fallback and existing layout/atlas limits stay the same.

Alternatives: parsing each probe repeats table work; persistent parsed faces require ownership/lifetime machinery beyond this change. Use a per-call OnceCell cache with references into the renderer's existing Arc<FontData> sources.

Implementation and validation:

- [x] Add a failing regression for lazy valid/invalid font parsing and reference reuse; preserve all existing shaping output tests.
- [x] Implement the local cache and route both font selection and shaping through it.
- [x] Run Windows/macOS tests, fmt/Clippy, Python suite, Windows native package and memory ceilings.
- [x] Download baseline/current packages and measure the same RTX3080Ti native output workload. Record exact hashes, CPU and memory with limits; do not infer physical display latency.
- Integration: PR [#28](https://github.com/safethecode/whostty/pull/28). Exact final-head review and merge/main CI evidence are recorded on the PR after checks pass.

Latest starting point is main86d7ce9, including PR26/27 memory optimizations. Earlier500MiB values describe an older binary. Source and hardware measurements must be distinguished. No Windows reboot is authorized. WSL remains uninstalled after the previous elevation cancellation.

## Regression and package verification

The new eight-cell `fifififi` regression retains four ligatures, probes an invalid font before the valid font and leaves a third fallback unused. On the baseline it parses 17 times, against an expected two, and fails in [RED Check 43393563908](https://github.com/safethecode/whostty/actions/runs/43393563908). The fixed version parses exactly twice, including the failed parse. The cache allocates transient slots for all unique sources but does not parse unused sources or retain faces beyond the call.

[Check 43395600221](https://github.com/safethecode/whostty/actions/runs/43395600221) at `75b1aaf` passes fmt, Clippy and workspace tests on both platforms: Windows 74 Rust tests, macOS 72, plus one ignored benchmark on each. Python runs 16 tests with two Windows/one macOS platform skips. Rust cannot be compiled locally on this PC; those results come from Actions.

The first regression-only commit failed fmt because the downloaded local rustfmt lacked its matching rustc_driver DLL. That failure is not the RED evidence above. After installing the matching verified toolchain DLL, the formatted regression reached the intended 17-versus-two assertion.

A package run also exposed a separate fixture issue: the 20-second/600-line-per-second workload could stop at 11,970 lines after a scheduler delay, while the existing memory verifier requires 12,000. `--minimum-lines` now continues paced output until the floor is met. Stop-file precedence and monitor stall/overall/shutdown limits remain enforced. A fake-clock regression fails at 60/120 lines before the fix and passes after it; stop requests are tested with an unmet floor.

[Package 43395600320](https://github.com/safethecode/whostty/actions/runs/43395600320) passes with 12,000 lines in both memory profiles. Late process-private medians are 58.33 MiB with zero scrollback and 87.62 MiB with 10,000 lines on the CI software DX12 adapter. Budget ceilings were not increased. Downloaded ZIP checksum, PE/static CRT, DPI manifest and font/dependency/ConPTY notices and matched runtime hashes pass local verification.

## RTX comparison

Windows 11 build 26100, NVIDIA GeForce RTX 3080 Ti, 16 logical CPUs. Fresh baseline package from main `86d7ce9` ([run 43391896966](https://github.com/safethecode/whostty/actions/runs/43391896966)) and candidate from `75b1aaf` use the same fixture SHA256, default font size 15, 10,000-line scrollback, ligatures enabled and compact GPU mode. Each run has eight idle seconds then 60 seconds at 600 lines/s, a 36,000-line minimum, alternate-screen switches every ten seconds and one-second resource samples. Runs were sequential: baseline Vulkan, baseline DX12, candidate Vulkan, candidate DX12.

| Backend | CPU median before → after | Late private MiB before → after | Late working set MiB before → after |
|---|---:|---:|---:|
| Vulkan | 23.07% → 15.63% | 273.47 → 272.99 | 217.24 → 225.95 |
| DX12 | 25.00% → 19.99% | 226.89 → 227.77 | 194.39 → 195.07 |

CPU uses one logical core as 100%, with the median across all app samples including startup/idle. Memory is the final 13-second median after 30-second monitor warmup. All four runs emitted 36,000 lines/4,464,073 source bytes, child exit zero, GUI alive at completion, no five-second progress stalls and no monitor errors. The owned GUI is forcibly closed only after recording completion; its final cleanup exit code of one is not a crash during the workload.

One run per combination, ordinary desktop load, foreground visibility and clocks not isolated: these are observed CPU reductions, not a statistically established speedup. Memory is effectively unchanged; working set also reflects residency. The test does not establish leak freedom or multi-hour stability. Earlier exploratory 120-second runs used the pre-floor fixture and are excluded from this matched comparison. Do not substitute CI software-adapter memory for these hardware values.

Candidate binary SHA256: `a58b959dbdda83839a540e422a2ab53a8f4a1cf80640239dc23161cad01f52eb`.
Baseline: `28dd0692389564332495d7ddb58f170fd9ac2ba2289e5588c1733937dca2659e`.
Shared fixture: `0dd3f1a2101d2c91370e5bbdd563480ab40df92cf50cb1fb9c402d9feb9ecd6a`.
Exact commits, metrics, native reports and limitations are in [the evidence JSON](shaping-face-reuse-2026-11-13.json); raw local runs are under ignored `dist/validation/memory-2026-11-13/comparison-*`.

## Native rendering and remaining checks

Eight native checks pass: Vulkan/DX12 × 1/1.25/1.5/2 render scale, including RTX adapter identity, actual PTY input roundtrip, shaped pixels and observation of the packaged ConPTY DLL loaded from the candidate directory. These are render-scale overrides on one native 100% display, not physical multi-monitor DPI movement.

At scale one, each backend also passes a ten-sample pipeline latency smoke test after 20 warmups. Write-to-GPU-readback p50/p95: Vulkan 99.50/135.69 ms; DX12 66.41/68.83 ms. Shell/PTY execution, UI scheduling and screenshot readback are included. These short samples do not measure physical key-to-photon latency, pure frame time or a before/after latency improvement.

WSL 3.0.5 x64 MSI was downloaded from Microsoft's official release, with matching published SHA256 and valid Microsoft Authenticode signature. The no-reboot installer handoff was canceled by Windows UAC before the helper started; WSL/features/Ubuntu were not installed and no reboot occurred. Remaining work includes WSL/Ubuntu validation once elevation is available, physical multi-monitor DPI/IME checks, multi-hour stability, repeated controlled performance/latency measurement, RTL/color emoji and advanced keyboard protocols. This change does not claim those areas complete.
