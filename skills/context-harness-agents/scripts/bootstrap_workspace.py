#!/usr/bin/env python3
"""Create a minimal source-scoped Context Harness agent workspace."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


IDENTIFIER = re.compile(r"^[A-Za-z0-9_-]+$")


def quoted(value: str) -> str:
    return json.dumps(value)


def checked_identifier(value: str, label: str) -> str:
    if not IDENTIFIER.fullmatch(value):
        raise SystemExit(f"{label} must contain only letters, digits, '_' or '-'")
    return value


def checked_root(workspace: Path, value: str) -> str:
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise SystemExit("content root must be a workspace-relative path")
    resolved = (workspace / path).resolve()
    if not resolved.is_dir() or not resolved.is_relative_to(workspace):
        raise SystemExit(f"content root is not a directory inside the workspace: {value}")
    return value


def write_new(path: Path, content: str, dry_run: bool) -> None:
    if path.exists():
        raise SystemExit(f"refusing to overwrite existing file: {path}")
    if dry_run:
        print(f"would create {path}")
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    print(f"created {path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", default=".")
    parser.add_argument("--agent", default="project-researcher")
    parser.add_argument("--content-root", default="docs")
    parser.add_argument("--source", default="project")
    parser.add_argument("--model-alias", default="default")
    parser.add_argument("--provider-model", required=True)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    workspace = Path(args.workspace).resolve()
    if not workspace.is_dir():
        raise SystemExit(f"workspace is not a directory: {workspace}")
    agent = checked_identifier(args.agent, "agent")
    source = checked_identifier(args.source, "source")
    model_alias = checked_identifier(args.model_alias, "model alias")
    content_root = checked_root(workspace, args.content_root)
    prefix = source.replace("_", "-")
    ctx_dir = workspace / ".ctx"

    config = f'''[db]
path = ".ctx/data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[embedding]
provider = "disabled"

[retrieval]
final_limit = 12
hybrid_alpha = 0.6
candidate_k_keyword = 80
candidate_k_vector = 80
group_by = "document"
doc_agg = "max"
max_chunks_per_doc = 3

[server]
bind = "127.0.0.1:7331"

[models.{model_alias}]
provider = "openai"
model = {quoted(args.provider_model)}
api_key_env = "OPENAI_API_KEY"

[connectors.filesystem.{source}]
root = {quoted(content_root)}
include_globs = ["**/*.md", "**/*.txt", "**/*.rs", "**/*.py", "**/*.ts", "**/*.tsx"]
exclude_globs = ["**/.git/**", "**/target/**", "**/node_modules/**", "**/.ctx/**"]
follow_symlinks = false
'''

    agent_resource = f'''[agent]
name = {quoted(agent)}
description = "Answers project questions from indexed evidence"
model = {quoted(model_alias)}
tools = [{quoted(prefix + ".search")}, {quoted(prefix + ".get")}]

[agent.execution]
max_turns = 8
timeout_seconds = 180

[agent.permissions]
mode = "read-only"

[prompt]
system = """
Search project context before answering factual questions. Use the get tool when
a search excerpt is not enough. Cite file paths or document titles for factual
claims. If the indexed evidence is incomplete, say what is missing instead of
guessing. Keep the answer focused on the user's requested task.
"""
'''

    search_tool = f'''schema_version = 1

[tool]
name = {quoted(prefix + ".search")}
implementation = "builtin.retrieval.search"
description = "Search indexed project files"

[config]
source = {quoted("filesystem:" + source)}

[fixed]
limit = 8

[restrictions]
sources = [{quoted("filesystem:" + source)}]
max_output_bytes = 65536
'''

    get_tool = f'''schema_version = 1

[tool]
name = {quoted(prefix + ".get")}
implementation = "builtin.retrieval.get"
description = "Read one indexed project document"

[config]
source = {quoted("filesystem:" + source)}

[restrictions]
sources = [{quoted("filesystem:" + source)}]
max_output_bytes = 131072
'''

    targets = [
        (ctx_dir / "config.toml", config),
        (ctx_dir / "agents" / f"{agent}.toml", agent_resource),
        (ctx_dir / "tools" / f"{source}-search.toml", search_tool),
        (ctx_dir / "tools" / f"{source}-get.toml", get_tool),
    ]
    existing = [str(path) for path, _ in targets if path.exists()]
    if existing:
        raise SystemExit("refusing partial setup; target files already exist:\n" + "\n".join(existing))
    for path, content in targets:
        write_new(path, content, args.dry_run)

    print("\nnext steps:")
    print("  ctx agent validate")
    print("  ctx tool bindings validate")
    print("  ctx init")
    print(f"  ctx sync filesystem:{source}")
    print(f"  ctx agent run {agent} \"Summarize the project and cite the evidence used.\"")


if __name__ == "__main__":
    main()
