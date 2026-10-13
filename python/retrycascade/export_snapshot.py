"""Export a small, verified public result snapshot without private raw files.

python -m retrycascade.export_snapshot --study ../output/my-study --destination ../results/my-study
"""

from __future__ import annotations

import argparse
import json
import shutil
from pathlib import Path

from retrycascade.followup_analysis import validate_trace
from retrycascade.pilot import digest_file, write_json
from retrycascade.study import audit_controls, load_runs, study_lock, verify_analysis

FIGURES = ("recovery_curves.png", "incident_outcomes.png")


def export_snapshot(study: Path, destination: Path) -> None:
    """Keep source artifact hashes, but never export host paths or git status."""
    study, destination = study.resolve(), destination.resolve()
    if (
        study == destination
        or study in destination.parents
        or destination in study.parents
    ):
        raise ValueError("snapshot destination must be separate from the raw study")
    with study_lock(study, shared=True):
        verify_analysis(study)
        plan, execution, runs = load_runs(study)
        if not plan.get("followup_protocol"):
            raise ValueError("snapshot requires a follow-up study")
        audit_controls(runs)
        for run in runs:
            validate_trace(run["summary"], run["config"])
        analysis = json.loads((study / "analysis_manifest.json").read_text())
        if analysis["run_count"] != len(runs):
            raise ValueError("analysis run count differs from verified raw runs")
        cells = json.loads((study / "cells.json").read_text())
        features = json.loads((study / "recovery_features.json").read_text())
        destination.mkdir(parents=True, exist_ok=True)
        for name in FIGURES:
            shutil.copyfile(study / name, destination / name)
        snapshot = {
            "snapshot_version": "retrycascade-public-results-v1",
            "claim_scope": "exploratory_synthetic_diagnostic_not_production_prediction",
            "run_count": len(runs),
            "feature_row_count": len(features),
            "discovery_seeds": plan["seeds"],
            "confirmation_seeds_used": plan["confirmation_seeds_used"],
            "study_protocol": plan["followup_protocol"],
            "cells": cells,
        }
        write_json(destination / "snapshot.json", snapshot)
        write_json(
            destination / "manifest.json",
            {
                "snapshot_version": snapshot["snapshot_version"],
                "source_plan_sha256": digest_file(study / "plan.json"),
                "source_analysis_manifest_sha256": digest_file(
                    study / "analysis_manifest.json"
                ),
                "source_simulator_binary_sha256": execution["binary_sha256"],
                "source_protocol_sha256": plan["protocol_sha256"],
                "followup_protocol_sha256": plan["followup_protocol_sha256"],
                "source_base_git_commit": execution["git_commit"],
                "source_worktree_was_dirty": bool(execution["git_status"]),
                "source_cargo_lock_sha256": execution["cargo_lock_sha256"],
                "source_python_lock_sha256": execution["python_lock_sha256"],
                "source_analysis_sha256": analysis["analysis_source_sha256"],
                "files": {
                    name: digest_file(destination / name)
                    for name in ("snapshot.json", *FIGURES)
                },
                "raw_artifacts_included": False,
                "note": "Pre-release study snapshot. Hashes identify the actual original study, not the later packaging commit. Re-running other platforms may not give bit-identical floating-point results.",
            },
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--study", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    export_snapshot(args.study, args.destination)
    print(f"Verified public snapshot: {args.destination}")


if __name__ == "__main__":
    main()
