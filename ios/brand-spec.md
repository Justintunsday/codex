# Native workspace design

Three directions were considered: a compact coding workspace, a spacious editorial conversation, and a warm minimal task dashboard. The implementation uses the coding workspace: task content gets most of the space, tools stay in native navigation, and file changes are readable at phone widths.

This is a source-port UI, not a reproduction of official brand assets. The Codex wordmark is text; no invented logo is used.

- Type: system sans for content, system serif for the empty-state title, system monospace for code.
- Accent: muted terracotta `B64D32` / `F09376` in dark appearance.
- Canvas: `FAF8F5` / `211F1D`; system primary and secondary text.
- Surface: `FDFCF9` / `2C2925` for the composer and user messages, with a quiet separator outline.
- Spacing: 4, 8, 16, 24, 32 and 48 points. Controls have at least a 44-point touch area.
- Native tabs on compact layouts and accessibility text sizes; workspace and session sidebar on regular layouts at standard text sizes.
- Dynamic Type uses semantic fonts. Code scrolls horizontally; prose wraps. No fixed screen dimensions.
- Diff is the signature detail: a focused native review with diff, before and after views and explicit save/reject controls.
- A new task starts with the selected project and at most three recent sessions. The composer provides a direct settings action and a compact runtime status; the thread has a 760-point reading width on larger devices.
- Terminal actions use SF Symbols with full accessibility labels so controls remain reachable at large text sizes. Connection settings distinguish ChatGPT account support from separately billed API access.

The frontend follows [wholiver's SwiftUI design skill](https://github.com/wholiver/swiftui-design-skill). Its newer-OS suggestions are applied only where they fit the required iOS 16 baseline. Design review evaluates layout, typography, color, brand coherence and accessibility against the actual simulator screenshots.

Screenshots for iPhone and iPad are captured by the UI test job. Visual and accessibility sign-off still require review of these artifacts and real-device checks on iOS 16, 17, 18 and 26.
