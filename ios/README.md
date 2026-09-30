# Native Codex iOS port — implementation status

This directory contains a real SwiftUI iPhone/iPad app and a Rust static-library bridge. The deployment target is iOS 16.0; device and simulator builds are arm64. Compatibility generations are 16, 17, 18 and 26. Business views only use the iOS 16 baseline.

**This is an incomplete port, not a finished replacement for the full Codex agent.** The working engine is a bounded mobile Responses adapter reusing `codex-api`, `codex-http-client` and `codex-protocol`. It does not yet embed `codex-core`, its desktop agent/session/config/tool orchestration, or its V8 code-mode runtime. The CI job separately attempts an actual `aarch64-apple-ios` check of the unchanged full core and uploads the result and platform inventory. A passing adapter build does not mean that full-core check passes.

## Current implementation

SwiftUI conversation and streamed output, reasoning summaries, tool activity, native file browser/editor, reviewed unified diffs, before/after comparison, API model discovery, Keychain API credentials, project import/share, persisted session history, and compatibility diagnostics. iPhone uses tabs; iPad uses a sidebar.

Projects are explicitly imported as app-owned copies through Document Picker and NSFileCoordinator. Rust never writes directly into an external file provider. Export edited files using Share. Imports reject symlinks and enforce a 10,000-file / 256 MiB limit. The editor supports UTF-8 files up to 64 KiB. File previews preserve a baseline and saving refuses stale changes. All paths are relative to the authorized project; symlinks and parent traversal are refused. Concurrent external mutation of the imported project is not supported.

The C ABI has six functions: version, initialize, command, poll event, free string and shutdown (version is a query). JSON commands cover create/restore/list sessions, prompt, cancel, lifecycle, project/files, preview/review and diagnostics. Handles are registry IDs, not borrowed Swift object pointers. Command/event queues are bounded (64/256). Rust strings are copied and freed before Swift decoding. Swift polls off the main thread. Tokio runs on a dedicated host thread with two workers. Panics are contained at the ABI/task boundaries; `panic=abort` builds are not supported.

Background and memory-pressure events cancel work; the app does not claim unlimited iOS background execution. Every text delta is checkpointed before delivery. Recovery appends failure results for interrupted tool calls and never automatically applies an unapproved write. Context is capped at 32 KiB, individual prompts at 8 KiB, assistant output at 32 KiB, and tool loops at 12 calls. The app reports limits and asks for a new session instead of silently rewriting context. Tool content can exceed 1,000 tokens and requires manual model-context review before merging.

Secrets are stored with Keychain `WhenUnlockedThisDeviceOnly`; no API key is persisted in Documents, preferences, Rust sessions, or diagnostics. Requests require HTTPS. ATS remains enabled; certificate verification and upstream TLS behavior remain intact. HTTP/2/SSE use the upstream transport. WebSocket support in the upstream dependency graph has not been exercised by the mobile engine.

## Not implemented yet

- Embedding full upstream core and preserving its exact agent behavior/configuration/rollout formats.
- Remote process/tool adapter, terminal emulator/PTY, native Git status/diff/commit.
- ChatGPT OAuth/account sign-in, refresh and account migration.
- Optional enhanced runtime: the capability boundary reports `adapterNotInstalled`; there is no privilege escalation or jailbreak implementation.
- In-place editing of external provider projects, project archive import/export, session deletion/export and resumable network tasks after background termination.
- Device validation on iOS 16, 17, 18 and 26, including Dynamic Type, rotation, background kills, TLS streaming and memory pressure.

The UI labels unsupported backends explicitly. It never assumes a shell, git, compiler, package manager, daemon manager or access to the root filesystem.

## GitHub build and installation

The `Native iOS` workflow builds on `macos-26`. It runs targeted Rust tests, builds the arm64 Rust static library, archives the native app, checks the iOS 16 deployment metadata, and runs screenshot-bearing UI smoke tests on an available iPhone and iPad simulator. Only installed simulator runtimes are exercised; this is not a four-version device test matrix.

Download `Codex-iOS` from a successful workflow run. Without signing secrets it contains `Codex-unsigned.app.zip` and `Codex-unsigned.ipa`. These are real compiled app bundles but **cannot be installed directly until signed**. No certificate or provisioning profile is fabricated.

For signed export, configure repository secrets `IOS_CERTIFICATE_BASE64` (PKCS#12), `IOS_CERTIFICATE_PASSWORD`, `IOS_PROFILE_BASE64`, and `IOS_TEAM_ID`. The provisioning profile must match `org.codex.native-ios` and include the intended devices for development/ad-hoc distribution. Set repository variable `IOS_EXPORT_METHOD` as appropriate for the chosen profile. Run the workflow manually with `sign=true`; the resulting signed IPA is uploaded in the same artifact. Do not commit credentials or put signing secrets in an issue or chat.

On a Mac, install Xcode, Rust 1.95 and `brew install xcodegen just`; run `bash ios/Scripts/build-app.sh`. XcodeGen creates `Codex.xcodeproj` from the committed specification. No WebView or CLI subprocess hosts the UI.

Rust's target requirements and deployment environment are documented in the [Rust iOS platform guide](https://doc.rust-lang.org/stable/rustc/platform-support/apple-ios.html). External directory authorization follows [Apple's document picker directory guidance](https://developer.apple.com/documentation/uikit/providing-access-to-directories).

## Reviewable landing stages

The complete application is larger than the repository's 800-line review guidance. The smallest coherent stage is the confined platform crate and its tests; subsequent stages are the persistence/Responses adapter and FFI, then native views and build automation. The current branch contains these stages for end-to-end compilation; split commits/PRs before upstream review. Existing CLI APIs and rollouts are unchanged, and existing desktop core modules are not modified.
