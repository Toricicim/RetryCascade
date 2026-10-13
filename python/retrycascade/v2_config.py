"""Versioned V2 resolver. The frozen V1 protocol and fixtures stay untouched."""

import json
from copy import deepcopy
from pathlib import Path

from retrycascade.protocol import (
    ProtocolError,
    _load_yaml,
    canonical_sha256,
    load_and_validate,
)
from retrycascade.resolve_config import resolve_run_config, validate_resolved_run_config

ROOT = Path(__file__).resolve().parents[2]


def validate_protocol(v2: dict) -> None:
    """Reject semantic labels the executable does not implement."""
    fixed = {
        "schema_version": "2.0.0",
        "protocol_version": "retrycascade-protocol-v2.0.0",
        "model_version": "retrycascade-des-v2.0.0",
        "claim_scope": "exploratory_synthetic_benchmark",
    }
    for key, expected in fixed.items():
        if v2.get(key) != expected:
            raise ProtocolError(f"unsupported V2 {key}")
    failure = v2["failure"]
    for key, expected in {
        "semantics": "pre_admission_retryable_failure",
        "sampling": "independent_per_potential_attempt",
        "namespace": "pre_admission_failure",
        "key": ["main_seed", "logical_request_id", "attempt_no"],
        "capacity_cost": "no_server_admission_or_service_work",
        "onset": "arrivals_begin",
        "end": "run_end",
    }.items():
        if failure.get(key) != expected:
            raise ProtocolError(f"unsupported failure.{key}")
    retry = v2["retry"]
    if (
        retry["potential_attempt_bank"] != 3
        or retry["max_attempts_including_initial"] != 3
    ):
        raise ProtocolError("V2 currently supports a three-attempt potential bank")
    if retry["budget_denial"] != "terminal_budget_denied_at_decision_time":
        raise ProtocolError("unsupported budget denial semantics")
    if retry["max_attempts"] != "terminal_attempts_exhausted_at_failure_notification":
        raise ProtocolError("unsupported attempts exhaustion semantics")
    factors = v2["discovery"]
    for values, low, high, name in (
        (failure["probabilities_basis_points"], 0, 10000, "failure probabilities"),
        (factors["rho_basis_points"], 1, 9999, "rho"),
        (factors["trigger_durations_us"], 0, 2**53 - 1, "trigger duration"),
        (retry["refill_basis_points"], 1, 10000, "refill ratio"),
    ):
        if (
            not values
            or len(values) != len(set(values))
            or any(
                type(value) is not int or not low <= value <= high for value in values
            )
        ):
            raise ProtocolError(f"invalid {name} factor levels")
    if (
        0 not in factors["trigger_durations_us"]
        or 0 not in failure["probabilities_basis_points"]
    ):
        raise ProtocolError("fault-free and no-trigger control levels are required")
    if not {"no_retry", "full_jitter", "retry_budget"}.issubset(factors["policies"]):
        raise ProtocolError("required no-retry/full-jitter/budget controls are missing")
    if v2["service_demand"]["noise"] != "none":
        raise ProtocolError("runtime noise is not implemented")
    # Canonical hashing also rejects float inputs throughout the document.
    canonical_sha256(v2)


def resolved_schema() -> dict:
    """Reuse V1 field contracts, with explicit V2 additions and versions."""
    schema = json.loads((ROOT / "schemas/resolved-run-config.schema.json").read_text())
    schema["$id"] = (
        "https://retrycascade.local/schemas/resolved-run-config-v2.schema.json"
    )
    schema["properties"]["schema_version"] = {"const": "2.0.0"}
    schema["properties"]["model_version"] = {"const": "retrycascade-des-v2.0.0"}
    schema["properties"]["protocol_version"] = {"const": "retrycascade-protocol-v2.0.0"}
    schema["required"].append("extensions")
    schema["properties"]["extensions"] = {
        "type": "object",
        "additionalProperties": False,
        "required": [
            "pre_admission_failure_basis_points",
            "failure_notification_delay_us",
            "demand_identity",
            "budget_denial_finishes_logical_request",
        ],
        "properties": {
            "pre_admission_failure_basis_points": {
                "type": "integer",
                "minimum": 0,
                "maximum": 10000,
            },
            "failure_notification_delay_us": {"$ref": "#/$defs/positive_tick"},
            "demand_identity": {"enum": ["request_stable", "independent_attempts"]},
            "budget_denial_finishes_logical_request": {"type": "boolean"},
            "accept_timed_out_completion": {"type": "boolean"},
            "observation": {
                "type": "object",
                "additionalProperties": False,
                "required": ["window_us", "reference_trigger_duration_us"],
                "properties": {
                    "window_us": {"const": 30000000},
                    "reference_trigger_duration_us": {"$ref": "#/$defs/tick"},
                },
            },
        },
    }
    retry = schema["properties"]["retry_contract"]["properties"]
    retry["max_attempts_including_initial"]["maximum"] = 3
    retry["retryable_causes"] = {
        "const": ["attempt_timeout", "queue_rejected", "pre_admission_failure"]
    }
    return schema


def load_v2_protocol(path: Path | None = None) -> tuple[dict, dict]:
    path = path or ROOT / "configs/research_protocol_v2.yaml"
    v2 = _load_yaml(path)
    validate_protocol(v2)
    base = load_and_validate(
        ROOT / "configs" / v2["base_protocol"],
        ROOT / "schemas/research-protocol.schema.json",
        kind="protocol",
    )
    if canonical_sha256(base) != v2["base_protocol_sha256"]:
        raise ProtocolError(
            "V1 base protocol changed; explicitly version the V2 dependency"
        )
    discovery = v2["discovery"]["seeds"]
    confirmation = v2["confirmation"]["seeds"]
    old_seeds = base["experiment"]["main_seeds"] + base["experiment"]["pilot_seeds"]
    if len(set(discovery + confirmation + old_seeds)) != len(
        discovery + confirmation + old_seeds
    ):
        raise ProtocolError(
            "discovery, confirmation and V1 seed banks must be disjoint"
        )
    return v2, base


def resolve_v2(
    v2: dict,
    base: dict,
    *,
    rho: int,
    probability: int,
    duration: int,
    policy: str,
    seed: int,
    refill_bp: int = 1000,
    timeout_us: int | None = None,
    demand_identity: str | None = None,
    reference_duration_us: int | None = None,
    accept_timed_out_completion: bool = False,
    budget_denial_finishes: bool = True,
) -> dict:
    if policy not in [
        "no_retry",
        "immediate",
        "exponential",
        "full_jitter",
        "retry_budget",
    ]:
        raise ProtocolError(f"unsupported V2 policy {policy}")
    if not 0 <= probability <= 10000 or not 0 < rho < 10000 or duration < 0:
        raise ProtocolError("invalid factor range")
    config = resolve_run_config(
        base,
        {
            # Resolve research-sized fields; V2 phase is managed by study manifest.
            "schema_version": base["schema_version"],
            "analysis_eligible": True,
            "seed": seed,
            "rho_basis_points": rho,
            "policy": "immediate" if policy == "no_retry" else policy,
            "trigger_enabled": duration > 0,
            "trigger_duration_us": duration,
        },
    )
    config.update(
        {
            "schema_version": v2["schema_version"],
            "model_version": v2["model_version"],
            "protocol_version": v2["protocol_version"],
            "analysis_eligible": False,
            "protocol_sha256": canonical_sha256(v2),
        }
    )
    config["retry_contract"]["retryable_causes"].append("pre_admission_failure")
    config["retry_contract"]["max_attempts_including_initial"] = (
        1 if policy == "no_retry" else v2["retry"]["max_attempts_including_initial"]
    )
    config["retry_contract"]["per_attempt_timeout_us"] = (
        timeout_us or v2["retry"]["timeout_us"]
    )
    config["retry_contract"]["overall_logical_deadline_us"] = v2["retry"]["deadline_us"]
    config["extensions"] = {
        "pre_admission_failure_basis_points": probability,
        "failure_notification_delay_us": v2["failure"]["notification_delay_us"],
        "demand_identity": demand_identity or v2["service_demand"]["identity"],
        "budget_denial_finishes_logical_request": budget_denial_finishes,
    }
    if accept_timed_out_completion:
        config["extensions"]["accept_timed_out_completion"] = True
    if reference_duration_us is not None:
        if type(reference_duration_us) is not int or reference_duration_us < 0:
            raise ProtocolError("invalid reference duration")
        if duration and duration != reference_duration_us:
            raise ProtocolError("active trigger must match its reference interval")
        config["extensions"]["observation"] = {
            "window_us": 30000000,
            "reference_trigger_duration_us": reference_duration_us,
        }
        if duration == 0:
            config["post_trigger_duration_us"] += reference_duration_us
            config["run_end_us"] += reference_duration_us
    if policy != "no_retry" and policy != "immediate":
        config["policy"]["base_us"] = v2["retry"]["base_us"]
        config["policy"]["cap_us"] = v2["retry"]["cap_us"]
    if policy == "retry_budget":
        if not 0 < refill_bp <= 10000:
            raise ProtocolError("invalid budget refill ratio")
        config["policy"]["token_bucket"].update(
            {
                "units_per_token": 10000,
                "retry_cost_units": 10000,
                "refill_units_per_unique_on_time_success": refill_bp,
                "capacity_units": 10000 * v2["retry"]["initial_retry_tokens"],
                "initial_units": 10000 * v2["retry"]["initial_retry_tokens"],
            }
        )
    config["run_config_sha256"] = canonical_sha256(config)
    validate_resolved_run_config(config, resolved_schema())
    baseline = config["observation"]
    if (
        baseline["baseline_end_us"] - baseline["baseline_start_us"]
        < config["retry_contract"]["overall_logical_deadline_us"]
    ):
        raise ProtocolError("baseline must contain a mature arrival cohort")
    return deepcopy(config)


def generate() -> None:
    from retrycascade.pilot import write_json

    v2, base = load_v2_protocol()
    write_json(ROOT / "schemas/resolved-run-config-v2.schema.json", resolved_schema())
    config = resolve_v2(
        v2,
        base,
        rho=5000,
        probability=1000,
        duration=0,
        policy="full_jitter",
        seed=v2["discovery"]["seeds"][0],
    )
    write_json(ROOT / "tests/golden/resolved_v2_config.json", config)


if __name__ == "__main__":
    generate()
