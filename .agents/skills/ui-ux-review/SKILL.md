---
name: ui-ux-review
description: Inspect and test HTML-test UI for visual regressions, accessibility, responsive layout and user flows. Use for screenshots, layout bugs, theme issues and UI acceptance.
---

# UI review
Inspired by [GitHub web-design-reviewer](https://github.com/github/awesome-copilot/tree/main/skills/web-design-reviewer); adapted for local Tauri.

1. Identify the affected screen, expected behavior and current build. Reproduce with synthetic data; capture before/after screenshots when tools are available.
2. Check light/dark palettes, hover/focus/selected/disabled states, readable text, keyboard navigation, accessible labels and visible focus. Target WCAG AA contrast (4.5:1 normal text, 3:1 large text and relevant UI boundaries).
3. Check minimum and normal window sizes, supported scales, long Russian labels, empty/loading/error states, overflow, dropdown placement, scroll position and sticky headers.
4. Exercise actual controls, keyboard and pointer interactions; check that filters, selections and pagination remain consistent. Do not treat a screenshot alone as functional proof.
5. Use existing Node/browser and installed Windows WebView2 tests; Playwright only if available and appropriate. Browser mocks do not prove native IPC/SMB behavior.
6. Report severity, reproduction, expected/actual, screenshot or test evidence and minimal fix. Recheck affected flows after edits; distinguish verified from untested. Review-only tasks do not authorize fixes.
