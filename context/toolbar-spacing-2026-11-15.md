# Toolbar spacing correction

R1: user-provided crop codex-clipboard-b7c3f0b1-f7e4-4dd8-bb8e-c1a28ed81b24.png, inspected directly. It shows the W mark, selected python.exe title, a smaller close target, then differently sized add/profile/preferences buttons. The full viewport and original display scale are unknown. The crop is defect evidence, not a target style to reproduce.

O1: existing code mixes selectable_label, small_button and normal/menu buttons inside a44px panel. Their padding and intrinsic heights differ. The dark panel can also contain controls using system light visuals. No root DESIGN.md exists.

D1/E1: keep the existing dark header and W mark, tab title/close grouping and all actions. Use a44pt header with10pt horizontal/8pt vertical inset,28pt control height,28×28 icon targets,4pt sibling gaps and8pt tab-group gaps. Use14pt labels,18pt W, centered contents, explicit dark header visuals and existing green selection accent. These are chosen egui logical-point values, not measurements inferred from crop pixels. Terminal font/theme remain separate.

Q1: compare actual allocated title/close/add/profile/preferences rectangles at1/1.25/1.5/2 render scale and light/dark host visuals; assert equal height, center, square action targets and balanced vertical insets. Preserve long titles, multiple tabs, menu choices and keyboard actions. Inspect a real rendered header before declaring visual alignment complete; tests alone are geometry evidence.

Plan: extract the existing header wrapper/control helpers without behavior changes, demonstrate the reported geometry failure, apply shared sizing/spacing/visuals, verify layout/interaction and native package, review and integrate through a separate PR. Use reference-decompose/reference-review for the user's correction. No whole-app redesign or unrelated features are part of this fix.

## Verification

RED0bfa9e2 [Check43612408361](https://github.com/safethecode/whostty/actions/runs/43612408361) reports title18pt/close14pt on the original helpers. The first sizing pass additionally exposed a14pt-wide profile menu target; an explicit28×28 custom menu button fixes it. An additional test-fixture unused response was caught by Clippy and explicitly discarded.

GREENdee7b11 [Check43613019829](https://github.com/safethecode/whostty/actions/runs/43613019829) passes both platforms, including the actual nested scroll layout at four scale factors/two host visuals, overflowing12long mixed-CJK titles at480/800/1120pt, and pointer press/release opening of the profile menu. Read-only independent review found no actionable issue in sizing, overflow, action mapping or menu behavior.

Visual QA uses the actual Whostty application, not a mock toolbar. The desktop observer can include unrelated occluding windows, so it is not accepted as after-image evidence. Explicit `WHOSTTY_UI_SNAPSHOT=<new .ppm path>` waits for a frame, writes one client GPU readback and exits that diagnostic application. Normal launches do nothing. Existing files are never overwritten; errors/timeouts are logged and the runner must validate the output. The15s timeout bounds the run.

`python scripts/capture_windows_ui.py --exe <portable whostty.exe> --output <new directory>` supplies an isolated config/Python fixture, validates dimensions/payload/dark header/selected-tab pixels and emits lossless client.png plus exact hashes. It uses stdlib only. Windows packaging captures this real application frame into the ui-snapshot artifact directory, alongside ordinary native/memory checks. This captures client pixels even while other windows overlap; it does not validate native OS titlebar appearance or physical monitor transitions.

Final-head checks, actual captured-image review and post-merge main evidence are recorded on PR30. Capture artifacts and local review records have their stated scope; this change does not claim complete hover/keyboard/accessibility or whole-product visual certification.
