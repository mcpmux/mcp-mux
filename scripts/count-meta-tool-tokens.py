#!/usr/bin/env python3
"""Measure mcpmux_* tools/list token budget (tiktoken cl100k_base when available)."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def tiktoken_count(text: str) -> int:
    try:
        import tiktoken  # type: ignore

        enc = tiktoken.get_encoding("cl100k_base")
        return len(enc.encode(text))
    except ImportError:
        return (len(text.encode("utf-8")) * 11 + 43) // 44


def run_rust_byte_report() -> dict[str, int]:
    proc = subprocess.run(
        [
            "cargo",
            "test",
            "-p",
            "mcpmux-gateway",
            "meta_tools_token_budget_report",
            "--",
            "--nocapture",
        ],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    combined = proc.stdout + proc.stderr
    match = re.search(
        r"META_TOOL_TOKEN_REPORT "
        r"core_bytes=(\d+) full_bytes=(\d+) "
        r"core_tiktoken=(\d+) full_tiktoken=(\d+) "
        r"core_claude_est=(\d+) full_claude_est=(\d+) "
        r"saved_tiktoken=(\d+) saved_claude_est=(\d+) "
        r"core_rmcp_bytes=(\d+) full_rmcp_bytes=(\d+)",
        combined,
    )
    if not match:
        print(combined, file=sys.stderr)
        raise RuntimeError("cargo test did not emit META_TOOL_TOKEN_REPORT (test failed?)")
    keys = [
        "core_bytes",
        "full_bytes",
        "core_tiktoken",
        "full_tiktoken",
        "core_claude_est",
        "full_claude_est",
        "saved_tiktoken",
        "saved_claude_est",
        "core_rmcp_bytes",
        "full_rmcp_bytes",
    ]
    return {k: int(v) for k, v in zip(keys, match.groups(), strict=True)}


def main() -> int:
    rust = run_rust_byte_report()
    print("Meta-tool tools/list token budget")
    print("--------------------------------")
    print(f"  Core (4 advertised):  {rust['core_tiktoken']} tiktoken (~{rust['core_claude_est']} Claude est.)")
    print(f"  Full (11 registered): {rust['full_tiktoken']} tiktoken (~{rust['full_claude_est']} Claude est.)")
    print(f"  Saved:                {rust['saved_tiktoken']} tiktoken (~{rust['saved_claude_est']} Claude est.)")
    print()
    print(f"  Serialized bytes — core: {rust['core_bytes']}, full: {rust['full_bytes']}")
    print()
    print(json.dumps(rust, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
