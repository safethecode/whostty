# Toolbar spacing correction

R1: user-provided crop codex-clipboard-b7c3f0b1-f7e4-4dd8-bb8e-c1a28ed81b24.png, inspected directly. It shows the W mark, selected python.exe title, a smaller close target, then differently sized add/profile/preferences buttons. The full viewport and original display scale are unknown. The crop is defect evidence, not a target style to reproduce.

O1: existing code mixes selectable_label, small_button and normal/menu buttons inside a44px panel. Their padding and intrinsic heights differ. The dark panel can also contain controls using system light visuals. No root DESIGN.md exists.

D1/E1: keep the existing dark header and W mark, tab title/close grouping and all actions. Use a44pt header with10pt horizontal/8pt vertical inset,28pt control height,28×28 icon targets,4pt sibling gaps and8pt tab-group gaps. Use14pt labels,18pt W, centered contents, explicit dark header visuals and existing green selection accent. These are chosen egui logical-point values, not measurements inferred from crop pixels. Terminal font/theme remain separate.

Q1: compare actual allocated title/close/add/profile/preferences rectangles at1/1.25/1.5/2 render scale and light/dark host visuals; assert equal height, center, square action targets and balanced vertical insets. Preserve long titles, multiple tabs, menu choices and keyboard actions. Inspect a real rendered header before declaring visual alignment complete; tests alone are geometry evidence.

Plan: extract the existing header wrapper/control helpers without behavior changes, demonstrate the reported geometry failure, apply shared sizing/spacing/visuals, verify layout/interaction and native package, review and integrate through a separate PR. Use reference-decompose/reference-review for the user's correction. No whole-app redesign or unrelated features are part of this fix.
