# Native workspace design

Three directions were considered: a compact coding workspace, a spacious editorial conversation, and a warm minimal task dashboard. The implementation uses the coding workspace: task content gets most of the space, tools stay in native navigation, and file changes are readable at phone widths.

This is a source-port UI, not a reproduction of official brand assets. The Codex wordmark is text; no invented logo is used.

- Type: system sans for content, system serif for the empty-state title, system monospace for code.
- Accent: muted terracotta `B64D32` / `F09376` in dark appearance.
- Canvas: `FAF8F5` / `211F1D`; system primary and secondary text.
- Spacing: 4, 8, 16, 24, 32 and 48 points. Controls have at least a 44-point touch area.
- Native tabs on compact layouts; workspace and session sidebar on regular layouts.
- Dynamic Type uses semantic fonts. Code scrolls horizontally; prose wraps. No fixed screen dimensions.
- Diff is the signature detail: a focused native review with diff, before and after views and explicit save/reject controls.

Screenshots for iPhone and iPad are captured by the UI test job. Visual and accessibility sign-off still require review of these artifacts and real-device checks on iOS 16, 17, 18 and 26.
