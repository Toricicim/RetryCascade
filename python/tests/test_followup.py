from copy import deepcopy

import pytest

from retrycascade.protocol import ProtocolError
from retrycascade.study import (
    GROUP_COLUMNS,
    MEASURES,
    attach_matched_controls,
    make_plan,
    metric_aggregates_sql,
    wilson_interval,
)
from retrycascade.v2_config import load_v2_protocol, resolve_v2


def test_followup_freezes_disjoint_seeds_and_calendar_aligned_controls():
    plan = make_plan("followup")
    assert len(plan["jobs"]) == 630
    assert plan["seeds"] == [910004, 910005, 910006, 910007, 910008]
    assert plan["confirmation_seeds_used"] is False
    jobs = plan["jobs"]
    first = jobs[0]
    control = first["config"]
    active = jobs[3]["config"]
    assert not control["trigger_enabled"] and active["trigger_enabled"]
    assert control["run_end_us"] == active["run_end_us"]
    assert control["extensions"]["observation"] == active["extensions"]["observation"]


def test_old_v2_default_is_unchanged_and_alternatives_are_explicit():
    protocol, base = load_v2_protocol()
    kwargs = {
        "rho": 5000,
        "probability": 1000,
        "duration": 0,
        "policy": "retry_budget",
        "seed": 910004,
    }
    original = resolve_v2(protocol, base, **kwargs)
    assert "observation" not in original["extensions"]
    assert "accept_timed_out_completion" not in original["extensions"]
    alternative = resolve_v2(
        protocol,
        base,
        **kwargs,
        reference_duration_us=10000000,
        accept_timed_out_completion=True,
        budget_denial_finishes=False,
    )
    assert alternative["extensions"]["accept_timed_out_completion"] is True
    assert alternative["extensions"]["budget_denial_finishes_logical_request"] is False
    assert original["run_config_sha256"] != alternative["run_config_sha256"]
    with pytest.raises(ProtocolError, match="match"):
        resolve_v2(
            protocol,
            base,
            **{**kwargs, "duration": 3000000},
            reference_duration_us=10000000,
        )


def test_matched_loss_uses_same_calendar_contract_and_not_overall_success():
    control = {key: 0 for key in GROUP_COLUMNS}
    control.update(
        reference_trigger_seconds=10,
        client_contract="strict_rpc",
        seed=910004,
        arm="budget_10pct",
        during_reference_success_ratio=0.99,
        early_post_success_ratio=0.98,
    )
    active = deepcopy(control)
    active.update(
        trigger_seconds=10,
        during_reference_success_ratio=0.10,
        early_post_success_ratio=0.80,
    )
    attach_matched_controls([control, active])
    assert active["matched_incident_excess_failure_ratio"] == pytest.approx(0.89)
    assert active["matched_early_post_excess_failure_ratio"] == pytest.approx(0.18)
    assert control["matched_incident_excess_failure_ratio"] == 0
    with pytest.raises(ValueError, match="control missing"):
        attach_matched_controls([active])


def test_small_seed_recovery_counts_do_not_imply_certainty():
    lower, upper = wilson_interval(3, 3)
    assert lower < 0.5 and upper == pytest.approx(1)
    assert wilson_interval(0, 0) == (None, None)
    with pytest.raises(ValueError):
        wilson_interval(4, 3)


def test_legacy_all_null_followup_fields_stay_unavailable(tmp_path):
    import duckdb

    from retrycascade.pilot import write_json

    row = {metric: None for metric in MEASURES}
    row["success_ratio"] = 1.0
    path = tmp_path / "legacy_rows.json"
    write_json(path, [row])
    with duckdb.connect() as db:
        values = db.execute(
            f"SELECT {metric_aggregates_sql()} FROM read_json_auto(?)", [str(path)]
        ).fetchone()
    assert values[0] == 1.0 and all(value is None for value in values[1:])
