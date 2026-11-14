# Nonblocking PTY resize — design and implementation plan

Goal: keep ConPTY resize and master destruction off the GUI thread, including during sustained output and repeated window changes. Existing code already releases the grid lock before resize, but still calls the native potentially blocking operation from Session::resize on the GUI thread.

Approach: move ownership of the master to the existing child lifecycle worker. A bounded wake channel and shared single latest-size slot coalesce resize requests. A separate closed bit in the same mailbox takes precedence over pending resizes. The GUI updates its terminal grid and logical size immediately; worker failures use existing SessionEvent::Error/wake. Natural child exit remains visible independently of event-queue capacity, and master cleanup still waits for pane close. No new worker per resize and no unbounded request queue.

Alternatives: one thread per resize risks unordered calls and growing thread counts; a synchronous mutex around the native master still blocks the GUI. Keep the existing child worker and its 100ms process polling, with immediate wake for resize/close. Tradeoff: native size is eventually applied; intermediate sizes can be skipped.

Ruling: continue in the existing clean codex/windows-runtime-hardening branch, following the user's main/branch workflow. The user has delegated prioritization, implementation, push/PR/review/merge; no additional design permission is required. No reboot is authorized.

Plan (writing-plans / executing-plans, inline execution):

- [ ] Add a deterministic blocking-native-resize regression: a fake master blocks resize until released; caller resize and drop must return without waiting, latest request wins, close discards pending work. Observe the old synchronous path fail.
- [ ] Introduce bounded coalescing lifecycle control and move native master ownership into the child worker; preserve native output/exit/drop tests.
- [ ] Run fmt/Clippy and Windows/macOS tests, Python suite and Windows package/native checks. Record actual CI and native results, without claiming physical DPI or multihour stability.
- [ ] Review the whole diff, commit/push, PR/self-review, merge only after green final-head checks; verify main and create a clean next branch.

Remaining priorities after this change: WSL (elevation canceled previously; no reboot), physical multi-monitor DPI/IME, multihour mixed workload, controlled GPU frame/physical latency measurement, RTL/color emoji/advanced keyboard protocols. PR28's RTX remeasurement is complete only within its documented single-run scope, not a remaining untouched task.
