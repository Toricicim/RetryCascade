"""Regenerate committed protocol hashes and resolved JSON fixtures."""

from __future__ import annotations

import json
from pathlib import Path

from retrycascade.protocol import canonical_sha256, load_and_validate
from retrycascade.resolve_config import (
    resolve_run_config,
    validate_resolved_run_config,
    write_resolved_run_config,
)


ROOT = Path(__file__).resolve().parents[2]


def _load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> None:
    protocol = load_and_validate(
        ROOT / "configs" / "research_protocol.yaml",
        ROOT / "schemas" / "research-protocol.schema.json",
        kind="protocol",
    )
    protocol_hash = canonical_sha256(protocol)
    (ROOT / "configs" / "research_protocol.sha256").write_text(
        f"{protocol_hash}\n",
        encoding="utf-8",
    )

    run_schema = ROOT / "schemas" / "run-config.schema.json"
    resolved_schema = _load_json(
        ROOT / "schemas" / "resolved-run-config.schema.json"
    )
    fixtures = (
        ("canonical_metastable.yaml", "resolved_run_config.json"),
        ("smoke.yaml", "resolved_smoke_config.json"),
    )
    for selection_name, output_name in fixtures:
        selection = load_and_validate(
            ROOT / "configs" / selection_name,
            run_schema,
            kind="run",
        )
        resolved = resolve_run_config(protocol, selection)
        validate_resolved_run_config(resolved, resolved_schema)
        write_resolved_run_config(
            ROOT / "tests" / "golden" / output_name,
            resolved,
        )


if __name__ == "__main__":
    main()
