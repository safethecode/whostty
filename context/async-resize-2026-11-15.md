# Nonblocking PTY resize — design and implementation plan

Goal: keep ConPTY resize and master destruction off the GUI thread, including during sustained output and repeated window changes. Existing code already releases the grid lock before resize, but still calls the native potentially blocking operation from Session::resize on the GUI thread.

Approach: move ownership of the master to the existing child lifecycle worker. A bounded wake channel and shared single latest-size slot coalesce resize requests. A separate closed bit in the same mailbox takes precedence over pending resizes. The GUI updates its terminal grid and logical size immediately; worker failures use existing SessionEvent::Error/wake. Natural child exit remains visible independently of event-queue capacity, and master cleanup still waits for pane close. No new worker per resize and no unbounded request queue.

Alternatives: one thread per resize risks unordered calls and growing thread counts; a synchronous mutex around the native master still blocks the GUI. Keep the existing child worker and its 100ms process polling, with immediate wake for resize/close. Tradeoff: native size is eventually applied; intermediate sizes can be skipped.

Ruling: continue in the existing clean codex/windows-runtime-hardening branch, following the user's main/branch workflow. The user has delegated prioritization, implementation, push/PR/review/merge; no additional design permission is required. No reboot is authorized.

Plan (writing-plans / executing-plans, inline execution):

- [x] Add a deterministic blocking-native-resize regression: a fake master blocks resize until released; caller resize and drop must return without waiting, latest request wins, close discards pending work. Observe the old synchronous path fail.
- [x] Introduce bounded coalescing lifecycle control and move native master ownership into the child worker; preserve native output/exit/drop tests.
- [x] Run fmt/Clippy and Windows/macOS tests, Python suite and Windows package/native checks. Record actual CI and native results, without claiming physical DPI or multihour stability.
- Integration: review, commits and pushes are on PR29. Exact final-head checks, native RTX smoke and merge/main verification are recorded on that PR as they finish; merge is gated on green checks.

Remaining priorities after this change: WSL (elevation canceled previously; no reboot), physical multi-monitor DPI/IME, multihour mixed workload, controlled GPU frame/physical latency measurement, RTL/color emoji/advanced keyboard protocols. PR28's RTX remeasurement is complete only within its documented single-run scope, not a remaining untouched task.

## Evidence and limits

RED: `e792d98`, [Check43610751395](https://github.com/safethecode/whostty/actions/runs/43610751395). Both Windows and macOS compile and fail the intended `Native resize blocked its caller` assertion after the injected three-second wait. This reproduces the synchronous-call hazard, not a claim that every native resize stalls for three seconds.

GREEN source: `b0ac08f`, [Check43610994811](https://github.com/safethecode/whostty/actions/runs/43610994811). Windows76/macOS74 Rust tests pass, with one ignored benchmark each, plus fmt/Clippy. Python16 tests pass with two Windows/one macOS platform skips. The test holds the first native call, issues1,000 requests, observes only the latest pending size, then closes while the next native call is blocked. The mailbox test also verifies a full wake slot cannot lose close or retain pending resize. Existing sustained-output resize, exit259, event-flood and live-session cleanup tests remain green.

The public spawn API is unchanged. A private master-decorator seam wraps an actual native PTY in the regression; readers/writers and child process are real. Successful Session::resize now acknowledges the grid update and queued request, not completed kernel resize. Native errors arrive asynchronously through the existing error event and wake mechanism. Intermediate requests may be skipped; no per-request threads or growing channel are introduced.

An already executing native resize cannot be canceled by this change and can delay worker exit polling/cleanup until it returns. The output reader remains separate and holds no control lock during the native call. GUI request/close do not wait for that call. This is not a physical multi-monitor DPI, key-to-photon latency, WSL or multihour reliability result.

Independent read-only whole-branch review found no actionable issue in synchronization, wake coalescing, close precedence, ownership, exit/reaping or error delivery. Final-head package/native and integration evidence will be recorded on [PR29](https://github.com/safethecode/whostty/pull/29) before/after merge.

Source [Windows package43610994797](https://github.com/safethecode/whostty/actions/runs/43610994797) passes: portable ZIP/static-runtime/notices verification, native PTY/shaped pixels at four scale overrides, input pipeline smoke and controlled memory ceilings. Local Python16 tests also pass (two platform skips), rustfmt1.88 and diff whitespace checks pass. Rust compilation/tests use Actions because no local Cargo/MSVC toolchain is installed. Hardware smoke results for the downloaded source package are kept in the PR comments to avoid changing the final source while integration CI is running.
