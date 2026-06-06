#!/usr/bin/env python3
"""Stop hook: enforce the AGENTS.md / CLAUDE.md mandatory-testing policy.

Blocks the agent from finishing when it edited CODE this session but did not run
the test suite afterward — or when the last test run after the edit FAILED.

Division of labor (see AGENTS.md):
  - This hook forces the suite to be RUN after code edits, and refuses to let the
    turn end on a failing run.
  - The policy (AGENTS.md §3/§4/§6) forces honesty about results (skipped ≠ green,
    paste literal evidence). The hook can't read the agent's mind, only its actions.

It is intentionally SATISFIABLE in every environment: running
`bash /opt/faro/scripts/test-all.sh` (which exits 0 even when it skips suites with
no toolchain) counts as "ran tests" and clears the block — so there is no
unescapable loop. Fail-open on any internal error: a buggy hook must never brick
the session.

Input: Stop-hook JSON on stdin (has `transcript_path`, `stop_hook_active`).
Output: exit 0 = allow stop; exit 2 + stderr = block and feed reason to the model.
"""

import json
import os
import re
import sys

CODE_EXT = (".rs", ".toml", ".ts", ".tsx", ".svelte", ".js", ".mjs",
            ".py", ".go", ".dart", ".kt", ".kts")

EDIT_TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}

# Substrings that mark a Bash command as a test run.
TEST_CMD_PATTERNS = (
    "test-all.sh", "cargo test", "cargo nextest", "npm test", "npm run test",
    "vitest", "pytest", "go test", "flutter test", "gradlew test", "gradle test",
    "node --test",
)

# Conservative failure markers (must NOT match a clean run like "0 failed").
FAILURE_RES = [
    re.compile(r"test result: FAILED"),
    re.compile(r"[1-9]\d* failed"),          # vitest/pytest/cargo "; N failed"
    re.compile(r"#\s*fail\s+[1-9]"),         # node --test "# fail 1"
    re.compile(r"\bBUILD FAILED\b"),
    re.compile(r"^FAIL\b", re.MULTILINE),    # go
    re.compile(r"fallaron:\s*[1-9]"),        # scripts/test-all.sh resumen
    re.compile(r"error\[E\d+\]"),            # rust compile error
    re.compile(r"No module named"),          # e.g. pytest not installed -> didn't run
    re.compile(r"command not found"),
]


def allow():
    sys.exit(0)


def block(msg: str):
    sys.stderr.write(msg)
    sys.exit(2)


def code_path(path: str) -> bool:
    if not path:
        return False
    p = path.replace("\\", "/")
    if p.endswith(".md"):
        return False
    if "/.claude/" in p or p.startswith(".claude/"):
        return False
    low = p
    in_backend = "/backend/" in low or low.startswith("backend/")
    in_cli = "/cli/" in low or low.startswith("cli/")
    in_frontend = "/frontend/" in low or low.startswith("frontend/")
    in_sdks = "/sdks/" in low or low.startswith("sdks/")
    if in_backend or in_cli:
        return p.endswith((".rs", ".toml"))
    if in_frontend:
        return p.endswith((".ts", ".tsx", ".svelte", ".js", ".mjs"))
    if in_sdks:
        return p.endswith(CODE_EXT)
    return False


def infer_suite(path: str) -> str:
    p = path.replace("\\", "/")
    if "/backend/" in p or p.startswith("backend/"):
        return "backend"
    if "/cli/" in p or p.startswith("cli/"):
        return "cli"
    if "/frontend/" in p or p.startswith("frontend/"):
        return "frontend"
    m = re.search(r"/sdks/([^/]+)/", p) or re.match(r"sdks/([^/]+)/", p)
    if m:
        return "sdk-" + m.group(1)
    return "?"


def is_test_cmd(cmd: str) -> bool:
    return any(pat in cmd for pat in TEST_CMD_PATTERNS)


def has_failure(text: str) -> bool:
    return any(rx.search(text) for rx in FAILURE_RES)


def result_text(content) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for it in content:
            if isinstance(it, dict):
                if isinstance(it.get("text"), str):
                    parts.append(it["text"])
                elif isinstance(it.get("content"), str):
                    parts.append(it["content"])
            elif isinstance(it, str):
                parts.append(it)
        return "\n".join(parts)
    return ""


def walk(obj, sink):
    if isinstance(obj, dict):
        sink(obj)
        for v in obj.values():
            walk(v, sink)
    elif isinstance(obj, list):
        for v in obj:
            walk(v, sink)


def main():
    raw = sys.stdin.read()
    try:
        data = json.loads(raw) if raw.strip() else {}
    except Exception:
        allow()
    tpath = data.get("transcript_path")
    if not tpath or not os.path.isfile(tpath):
        allow()

    uses = []          # ordered: {"pos","name","input","id"}
    results = {}       # id -> text
    counter = [0]

    with open(tpath, "r", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except Exception:
                continue

            def sink(o):
                t = o.get("type")
                if t == "tool_use" and "name" in o:
                    uses.append({
                        "pos": counter[0],
                        "name": o.get("name"),
                        "input": o.get("input") or {},
                        "id": o.get("id"),
                    })
                    counter[0] += 1
                elif t == "tool_result" and "tool_use_id" in o:
                    results[o.get("tool_use_id")] = result_text(o.get("content"))

            walk(msg, sink)

    # Code edits (with position + path).
    code_edits = []
    for u in uses:
        if u["name"] in EDIT_TOOLS:
            path = u["input"].get("file_path") or u["input"].get("notebook_path") or ""
            if code_path(path):
                code_edits.append((u["pos"], path))
    if not code_edits:
        allow()  # no code touched this session

    last_edit_pos = max(p for p, _ in code_edits)

    # Test runs (Bash commands matching a test pattern).
    test_runs = [u for u in uses
                 if u["name"] == "Bash" and is_test_cmd(u["input"].get("command", ""))]
    tests_after = [u for u in test_runs if u["pos"] > last_edit_pos]
    last_test_pos = max((u["pos"] for u in test_runs), default=-1)

    uncovered = sorted({(infer_suite(path), path)
                        for pos, path in code_edits if pos > last_test_pos})

    if not tests_after:
        suites = sorted({s for s, _ in uncovered})
        files = "\n".join(f"  - {path}  → suite: {s}" for s, path in uncovered)
        cmd = "bash /opt/faro/scripts/test-all.sh " + " ".join(suites)
        block(
            "⛔ BLOQUEADO por la política de testing (AGENTS.md / CLAUDE.md).\n\n"
            "Editaste código y NO corriste los tests después:\n"
            f"{files}\n\n"
            "Antes de terminar, corré los tests de las suites afectadas y pegá su\n"
            "salida real (AGENTS.md §3):\n\n"
            f"  {cmd}\n\n"
            "Si falta un toolchain, corré igual `bash /opt/faro/scripts/test-all.sh`\n"
            "(sale 0 saltando suites) y declará las saltadas (AGENTS.md §6). Pegá la\n"
            "cola literal + el bloque RESUMEN como evidencia.\n"
        )

    # A test ran after the last edit — did the most recent one fail?
    last_after = max(tests_after, key=lambda u: u["pos"])
    text = results.get(last_after["id"], "")
    if has_failure(text):
        cmd = last_after["input"].get("command", "")
        block(
            "⛔ BLOQUEADO: el último run de tests después de tu edición FALLÓ "
            "(o el toolchain no corrió).\n\n"
            f"Comando: {cmd}\n\n"
            "Arreglá el código (o el test si el contrato cambió a propósito) y volvé\n"
            "a correr hasta verde real. No silencies, borres ni filtres tests, ni uses\n"
            "`|| true` (AGENTS.md §3.4). Cuando pase, pegá la salida literal.\n"
        )

    allow()


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception:
        # Fail-open: never brick the session because of a hook bug.
        allow()
