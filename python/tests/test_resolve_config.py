import json
from copy import deepcopy
from pathlib import Path
import pytest

from retrycascade.protocol import ProtocolError, load_and_validate
from retrycascade.resolve_config import (
    U64_MAX,
    resolve_run_config,
    validate_resolved_run_config,
    write_resolved_run_config,
)

ROOT = Path(__file__).resolve().parents[2]

PROTOCOL_PATH = ROOT / "configs" / "research_protocol.yaml"
PROTOCOL_SCHEMA = ROOT / "schemas" / "research-protocol.schema.json"

SELECTION_PATH = ROOT / "configs" / "canonical_metastable.yaml"
RUN_SCHEMA = ROOT / "schemas" / "run-config.schema.json"
RESOLVED_SCHEMA_PATH = ROOT / "schemas" / "resolved-run-config.schema.json"
GOLDEN_PATH = ROOT / "tests" / "golden" / "resolved_run_config.json"


def load_resolved_schema() -> dict:
    return json.loads(RESOLVED_SCHEMA_PATH.read_text(encoding="utf-8"))


def test_resolver_combines_protocol_and_selection() -> None:
    protocol = load_and_validate(
        PROTOCOL_PATH,
        PROTOCOL_SCHEMA,
        kind="protocol",
    )
    selection = load_and_validate(
        SELECTION_PATH,
        RUN_SCHEMA,
        kind="run",
    )

    resolved = resolve_run_config(protocol, selection)
    validate_resolved_run_config(resolved, load_resolved_schema())

    assert resolved["protocol_version"] == protocol["protocol_version"]
    assert resolved["seed"] == selection["seed"]
    assert resolved["rho_basis_points"] == 9000
    assert resolved["policy"] == {
        "kind": "immediate",
        "epsilon_us": 1,
    }
    assert resolved["trigger_enabled"] is True
    assert resolved["trigger_duration_us"] == 10_000_000
    assert [stage["name"] for stage in resolved["stages"]] == [
        "source",
        "transform",
        "sink",
    ]
    assert resolved["stages"][0]["nominal_rate_work_units_per_second"] == 140
    assert resolved["stages"][1]["nominal_rate_work_units_per_second"] == 120
    assert resolved["stages"][2]["nominal_rate_work_units_per_second"] == 100

    retry_contract = resolved["retry_contract"]

    assert retry_contract["per_attempt_timeout_us"] == 500_000
    assert retry_contract["overall_logical_deadline_us"] == 2_000_000
    assert retry_contract["max_attempts_including_initial"] == 3
    assert retry_contract["retryable_causes"] == ["attempt_timeout", "queue_rejected"]

    assert resolved["trigger_start_us"] == 300_000_000
    assert resolved["trigger_end_us"] == 310_000_000
    assert resolved["trigger_capacity_multiplier_basis_points"] == 5000
    assert resolved["post_trigger_duration_us"] == 900_000_000
    assert resolved["run_end_us"] == 1_210_000_000

    assert resolved["arrival_process"]["distribution"] == "poisson"

    assert resolved["service_demand"]["distribution"] == "lognormal"
    assert resolved["service_demand"]["mean_work_units_milli"] == 1000
    assert resolved["service_demand"]["coefficient_of_variation_milli"] == 500

    assert resolved["request_semantics"]["cancellation_after_timeout"] is False
    assert (
        resolved["request_semantics"]["late_timed_out_completion_produces_goodput"]
        is False
    )
    assert resolved["observation"] == {
        "arrivals_begin_us": 0,
        "warmup_end_us": 180_000_000,
        "baseline_start_us": 180_000_000,
        "baseline_end_us": 300_000_000,
    }
    assert resolved["recovery"]["window_us"] == 30_000_000
    assert resolved["recovery"]["consecutive_non_overlapping_windows"] == 3
    assert resolved["recovery"]["goodput_fraction_of_g0_basis_points"] == 9500
    assert resolved["maximum_events"] == 20_000_000
    assert resolved["rng"]["arrival_key"] == [
        "main_seed",
        "logical_arrival_index",
    ]
    assert resolved["rng"]["service_key"] == [
        "main_seed",
        "logical_request_id",
        "attempt_no",
        "stage",
    ]
    assert resolved["rng"]["policy_must_not_change_arrival_trace"] is True
    assert len(resolved["protocol_sha256"]) == 64
    assert len(resolved["run_config_sha256"]) == 64

    resolved_again = resolve_run_config(protocol, selection)
    assert resolved_again == resolved


@pytest.mark.parametrize(
    ("policy_name", "expected_policy"),
    [
        (
            "immediate",
            {
                "kind": "immediate",
                "epsilon_us": 1,
            },
        ),
        (
            "exponential",
            {
                "kind": "exponential",
                "base_us": 25_000,
                "cap_us": 200_000,
            },
        ),
        (
            "full_jitter",
            {
                "kind": "full_jitter",
                "base_us": 25_000,
                "cap_us": 200_000,
                "rng_namespace": "retry_jitter",
            },
        ),
        (
            "retry_budget",
            {
                "kind": "retry_budget",
                "base_us": 25_000,
                "cap_us": 200_000,
                "token_bucket": {
                    "units_per_token": 10,
                    "retry_cost_units": 10,
                    "refill_units_per_unique_on_time_success": 1,
                    "capacity_units": 100,
                    "initial_units": 100,
                    "token_denial_result": "retry_denied",
                },
            },
        ),
    ],
)
def test_resolver_selects_only_requested_policy_config(
    policy_name: str,
    expected_policy: dict,
) -> None:
    protocol = load_and_validate(
        PROTOCOL_PATH,
        PROTOCOL_SCHEMA,
        kind="protocol",
    )
    selection = load_and_validate(
        SELECTION_PATH,
        RUN_SCHEMA,
        kind="run",
    )
    selection["policy"] = policy_name

    resolved = resolve_run_config(protocol, selection)

    assert resolved["policy"] == expected_policy


def test_run_hash_changes_but_protocol_hash_does_not() -> None:
    protocol = load_and_validate(PROTOCOL_PATH, PROTOCOL_SCHEMA, kind="protocol")
    selection = load_and_validate(SELECTION_PATH, RUN_SCHEMA, kind="run")

    first = resolve_run_config(protocol, selection)
    changed_selection = deepcopy(selection)
    changed_selection["seed"] += 1
    second = resolve_run_config(protocol, changed_selection)

    assert first["protocol_sha256"] == second["protocol_sha256"]
    assert first["run_config_sha256"] != second["run_config_sha256"]


def test_resolved_schema_rejects_unknown_field() -> None:
    protocol = load_and_validate(PROTOCOL_PATH, PROTOCOL_SCHEMA, kind="protocol")
    selection = load_and_validate(SELECTION_PATH, RUN_SCHEMA, kind="run")
    resolved = resolve_run_config(protocol, selection)
    resolved["unknown_field"] = True

    with pytest.raises(ProtocolError, match="Additional properties"):
        validate_resolved_run_config(resolved, load_resolved_schema())


def test_time_overflow_is_rejected() -> None:
    protocol = load_and_validate(PROTOCOL_PATH, PROTOCOL_SCHEMA, kind="protocol")
    selection = load_and_validate(SELECTION_PATH, RUN_SCHEMA, kind="run")
    protocol["trigger"]["start_us"] = U64_MAX

    with pytest.raises(ProtocolError, match="trigger_end_us"):
        resolve_run_config(protocol, selection)


def test_resolved_config_matches_golden_fixture() -> None:
    protocol = load_and_validate(PROTOCOL_PATH, PROTOCOL_SCHEMA, kind="protocol")
    selection = load_and_validate(SELECTION_PATH, RUN_SCHEMA, kind="run")
    resolved = resolve_run_config(protocol, selection)
    golden = json.loads(GOLDEN_PATH.read_text(encoding="utf-8"))

    assert resolved == golden


def test_resolved_hash_mismatch_is_rejected() -> None:
    protocol = load_and_validate(PROTOCOL_PATH, PROTOCOL_SCHEMA, kind="protocol")
    selection = load_and_validate(SELECTION_PATH, RUN_SCHEMA, kind="run")
    resolved = resolve_run_config(protocol, selection)
    resolved["seed"] += 1

    with pytest.raises(ProtocolError, match="run_config_sha256 mismatch"):
        validate_resolved_run_config(resolved, load_resolved_schema())


def test_resolved_config_writer_round_trips(tmp_path: Path) -> None:
    protocol = load_and_validate(PROTOCOL_PATH, PROTOCOL_SCHEMA, kind="protocol")
    selection = load_and_validate(SELECTION_PATH, RUN_SCHEMA, kind="run")
    resolved = resolve_run_config(protocol, selection)
    output_path = tmp_path / "resolved_run_config.json"

    write_resolved_run_config(output_path, resolved)

    assert json.loads(output_path.read_text(encoding="utf-8")) == resolved
