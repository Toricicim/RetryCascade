import json
import subprocess
from pathlib import Path
from unittest.mock import patch

import pytest

from retrycascade.pilot import write_json
from retrycascade.protocol import ProtocolError, canonical_sha256
from retrycascade.study import execute_job, freeze_plan, verify_artifact
from retrycascade.v2_config import load_v2_protocol, resolve_v2


def test_v2_resolver_separates_versions_and_preserves_budget_capacity():
    protocol, base = load_v2_protocol()
    first = resolve_v2(
        protocol,
        base,
        rho=5000,
        probability=1000,
        duration=0,
        policy="retry_budget",
        seed=910001,
        refill_bp=500,
    )
    second = resolve_v2(
        protocol,
        base,
        rho=5000,
        probability=1000,
        duration=0,
        policy="retry_budget",
        seed=910001,
        refill_bp=2500,
    )
    assert first["schema_version"] == "2.0.0"
    assert base["schema_version"] == "1.1.0"
    assert first["run_config_sha256"] == canonical_sha256(first)
    assert (
        first["policy"]["token_bucket"]["capacity_units"]
        == second["policy"]["token_bucket"]["capacity_units"]
    )
    assert first["run_config_sha256"] != second["run_config_sha256"]
    with pytest.raises(ProtocolError):
        resolve_v2(
            protocol,
            base,
            rho=5000,
            probability=10001,
            duration=0,
            policy="full_jitter",
            seed=910001,
        )


def test_frozen_plan_and_corrupt_completed_artifact_fail_closed(tmp_path: Path):
    plan = {"seeds": [1], "jobs": []}
    freeze_plan(tmp_path, plan)
    freeze_plan(tmp_path, plan)
    with pytest.raises(ValueError, match="plan differs"):
        freeze_plan(tmp_path, {"seeds": [2], "jobs": []})
    # Completed run with a truncated/missing summary is never resumed silently.
    write_json(
        tmp_path / "manifest.json",
        {
            "status": "complete",
            "binary_sha256": "same",
            "files": {"plan.json": "wrong-sha256"},
        },
    )
    with pytest.raises(ValueError, match="checksum mismatch"):
        verify_artifact(tmp_path, {}, "same")


def test_infrastructure_failure_is_retained_and_can_be_retried(tmp_path: Path):
    root = Path(__file__).resolve().parents[2]
    config = json.loads((root / "tests/golden/resolved_v2_config.json").read_text())
    summary = (root / "tests/golden/run_summary_v2.json").read_text()
    job = {"config": config, "metadata": {"phase": "test"}}
    binary = Path("mock-simulator")
    directory = tmp_path / "runs" / config["run_config_sha256"]
    with (
        patch(
            "retrycascade.study.subprocess.run",
            side_effect=subprocess.TimeoutExpired("sim", 180),
        ),
        pytest.raises(subprocess.TimeoutExpired),
    ):
        execute_job(job, binary, tmp_path, "mock-hash")
    assert json.loads((directory / "failure.json").read_text())["status"] == "failed"
    assert not (directory / "manifest.json").exists()
    with pytest.raises(ValueError, match="previous failure"):
        execute_job(job, binary, tmp_path, "mock-hash")
    process = subprocess.CompletedProcess([], 0, stdout=summary, stderr="")
    with patch("retrycascade.study.subprocess.run", return_value=process):
        execute_job(job, binary, tmp_path, "mock-hash", retry_failed=True)
    assert (directory / "failure.json").exists()
    assert (directory / "failure_resolution.json").exists()
    assert (
        verify_artifact(directory, config, "mock-hash")["summary_schema_version"]
        == "retrycascade-run-summary-v2.0.0"
    )
    with patch("retrycascade.study.subprocess.run") as run:
        assert execute_job(job, binary, tmp_path, "mock-hash")["resumed"]
        run.assert_not_called()
