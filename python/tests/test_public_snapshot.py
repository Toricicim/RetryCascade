import json
from pathlib import Path

from retrycascade.pilot import digest_file

SNAPSHOT = Path(__file__).resolve().parents[2] / "results/followup-2026-10-13"


def test_public_snapshot_files_match_their_source_manifest():
    manifest = json.loads((SNAPSHOT / "manifest.json").read_text())
    assert manifest["raw_artifacts_included"] is False
    for name, expected in manifest["files"].items():
        assert digest_file(SNAPSHOT / name) == expected
    data = json.loads((SNAPSHOT / "snapshot.json").read_text())
    assert data["run_count"] == 630 and data["feature_row_count"] == 27090
    assert data["confirmation_seeds_used"] is False
    assert data["discovery_seeds"] == [910004, 910005, 910006, 910007, 910008]


def test_readme_example_is_a_tradeoff_not_a_universal_winner():
    data = json.loads((SNAPSHOT / "snapshot.json").read_text())
    selected = {
        c["arm"]: c
        for c in data["cells"]
        if c["rho"] == 0.575
        and c["failure_rate"] == 0.1
        and c["trigger_seconds"] == 10
        and c["client_contract"] == "strict_rpc"
    }
    budget, no_retry = selected["budget_10pct"], selected["no_retry"]
    assert (
        budget["during_reference_success_ratio"]
        < no_retry["during_reference_success_ratio"]
    )
    assert budget["early_post_success_ratio"] > no_retry["early_post_success_ratio"]
    assert selected["full_jitter"]["recovery_count"] == 0
