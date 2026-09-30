"""Inventory platform-sensitive sites; static matches are candidates, not proven defects."""

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RULES = {
    "os_gate": r"target_os\s*=|#\[cfg\(unix\)|target_family",
    "process": r"Command::new|tokio::process|std::process|spawn_exec|subprocess",
    "pty_signal": r"portable.pty|termios|openpty|signal_hook|tokio::signal|libc::(fork|kill|setsid)",
    "sandbox": r"seatbelt|sandbox.exec|landlock|seccomp|mxc|bwrap",
    "network_tls": r"native.tls|rustls|reqwest|tungstenite|Security.framework|system.configuration",
    "filesystem_paths": r"std::fs|tokio::fs|std::os::unix|CARGO_MANIFEST_DIR|/usr/bin|/bin/bash",
    "environment_terminal": r"std::env|env::var|which::|IsTerminal|crossterm",
    "jit": r"\bv8\b|deno_core|MAP_JIT",
}
report = {name: [] for name in RULES}
for path in sorted((ROOT / "codex-rs").rglob("*")):
    if "target" in path.parts or "vendor" in path.parts:
        continue
    if path.suffix != ".rs" and path.name != "Cargo.toml":
        continue
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        for category, pattern in RULES.items():
            if re.search(pattern, line):
                report[category].append({"file": path.relative_to(ROOT).as_posix(), "line": number, "source": line.strip()[:240]})
output = ROOT / "ios/Build/Audit"
output.mkdir(parents=True, exist_ok=True)
(output / "platform-sites.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
summary = ["# iOS platform candidates", "", "Static inventory only; use the arm64 core check log to identify compile blockers.", ""]
summary.extend(f"- {category}: {len(matches)} sites" for category, matches in report.items())
(output / "platform-summary.md").write_text("\n".join(summary) + "\n", encoding="utf-8")
print("\n".join(summary))
