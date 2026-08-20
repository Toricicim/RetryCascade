"""Build a complete single-run configuration for the Rust simulator."""

from __future__ import annotations

import json
from copy import deepcopy
from pathlib import Path
from typing import Any

import jsonschema

from retrycascade.protocol import ProtocolError, canonical_sha256


U64_MAX = 2**64 - 1


def _checked_add_us(left: int, right: int, *, field: str) -> int:
    result = left + right
    if left < 0 or right < 0 or result > U64_MAX:
        raise ProtocolError(f"{field} exceeds the unsigned 64-bit time domain")
    return result


def _resolve_policy(
    protocol: dict[str, Any],
    policy_name: str,
) -> dict[str, Any]:
    policies = protocol["policies"]

    if policy_name == "immediate":
        return {
            "kind": "immediate",
            "epsilon_us": policies["immediate"]["epsilon_us"],
        }
    if policy_name == "exponential":
        return {
            "kind": "exponential",
            "base_us": policies["common"]["exponential_base_us"],
            "cap_us": policies["common"]["exponential_cap_us"],
        }
    if policy_name == "full_jitter":
        return {
            "kind": "full_jitter",
            "base_us": policies["common"]["exponential_base_us"],
            "cap_us": policies["common"]["exponential_cap_us"],
            "rng_namespace": policies["full_jitter"]["jitter_rng_namespace"],
        }
    if policy_name == "retry_budget":
        return {
            "kind": "retry_budget",
            "base_us": policies["common"]["exponential_base_us"],
            "cap_us": policies["common"]["exponential_cap_us"],
            "token_bucket": policies["retry_budget"]["token_bucket_fixed_point"],
        }
    raise ValueError(f"Unsupported policy: {policy_name}")


def resolve_run_config(
    protocol: dict[str, Any],
    selection: dict[str, Any],
) -> dict[str, Any]:
    """Combine the research protocol and one factor selection."""

    analysis_eligible = selection["analysis_eligible"]
    trigger_start_us = (
        protocol["trigger"]["start_us"]
        if analysis_eligible
        else selection["trigger_start_us"]
    )
    trigger_duration_us = selection["trigger_duration_us"]
    post_trigger_duration_us = (
        protocol["observation"]["post_trigger_artifact_horizon_us"]
        if analysis_eligible
        else selection["post_trigger_horizon_us"]
    )
    trigger_end_us = _checked_add_us(
        trigger_start_us,
        trigger_duration_us,
        field="trigger_end_us",
    )
    run_end_us = _checked_add_us(
        trigger_end_us,
        post_trigger_duration_us,
        field="run_end_us",
    )

    policy_name = selection["policy"]
    policy_config = _resolve_policy(protocol, policy_name)

    stages = deepcopy(protocol["topology"]["stages"])
    retry_contract = deepcopy(protocol["retry_contract"])
    recovery = deepcopy(protocol["recovery"])

    if not analysis_eligible:
        for stage in stages:
            stage["queue_capacity_waiting"] = selection["queue_capacity_waiting"]
        retry_contract["per_attempt_timeout_us"] = selection[
            "per_attempt_timeout_us"
        ]
        retry_contract["overall_logical_deadline_us"] = selection[
            "overall_logical_deadline_us"
        ]
        retry_contract["max_attempts_including_initial"] = selection[
            "max_attempts_including_initial"
        ]
        recovery["window_us"] = selection["recovery_window_us"]

    observation = (
        {
            "arrivals_begin_us": protocol["observation"]["arrivals_begin_us"],
            "warmup_end_us": protocol["observation"]["warmup_end_us"],
            "baseline_start_us": protocol["observation"]["baseline_start_us"],
            "baseline_end_us": protocol["observation"]["baseline_end_us"],
        }
        if analysis_eligible
        else {
            "arrivals_begin_us": 0,
            "warmup_end_us": selection["warmup_end_us"],
            "baseline_start_us": selection["baseline_start_us"],
            "baseline_end_us": trigger_start_us,
        }
    )

    resolved = {
        "rng": protocol["rng"],
        "schema_version": selection["schema_version"],
        "analysis_eligible": analysis_eligible,
        "protocol_version": protocol["protocol_version"],
        "model_version": protocol["model_version"],
        "rng_version": protocol["rng_version"],
        "seed": selection["seed"],
        "rho_basis_points": selection["rho_basis_points"],
        "arrival_process": {
            "distribution": protocol["workload"]["arrival_process"]["distribution"],
            "rate_definition": protocol["workload"]["arrival_process"][
                "rate_definition"
            ],
        },
        "service_demand": protocol["workload"]["service_demand"],
        "request_semantics": protocol["workload"]["request_semantics"],
        "policy": policy_config,
        "trigger_enabled": selection["trigger_enabled"],
        "trigger_duration_us": selection["trigger_duration_us"],
        "stages": stages,
        "retry_contract": retry_contract,
        "trigger_start_us": trigger_start_us,
        "trigger_end_us": trigger_end_us,
        "trigger_capacity_multiplier_basis_points": protocol["trigger"][
            "capacity_multiplier_basis_points"
        ],
        "post_trigger_duration_us": post_trigger_duration_us,
        "run_end_us": run_end_us,
        "observation": observation,
        "recovery": recovery,
        "maximum_events": (
            protocol["safety"]["maximum_events_per_run"]
            if analysis_eligible
            else selection["maximum_events"]
        ),
    }

    resolved["protocol_sha256"] = canonical_sha256(protocol)
    resolved["run_config_sha256"] = canonical_sha256(resolved)
    return resolved


def validate_resolved_run_config(
    resolved: dict[str, Any],
    schema: dict[str, Any],
) -> None:
    try:
        jsonschema.Draft202012Validator(schema).validate(resolved)
    except jsonschema.ValidationError as exc:
        raise ProtocolError(f"resolved run config: {exc.message}") from exc

    retry = resolved["retry_contract"]
    if retry["per_attempt_timeout_us"] >= retry["overall_logical_deadline_us"]:
        raise ProtocolError("resolved run config: timeout must be less than deadline")

    stages = resolved["stages"]
    rates = [stage["nominal_rate_work_units_per_second"] for stage in stages]
    if rates[-1] != min(rates):
        raise ProtocolError("resolved run config: sink must be the bottleneck")

    expected_trigger_end = _checked_add_us(
        resolved["trigger_start_us"],
        resolved["trigger_duration_us"],
        field="trigger_end_us",
    )
    expected_run_end = _checked_add_us(
        expected_trigger_end,
        resolved["post_trigger_duration_us"],
        field="run_end_us",
    )
    if resolved["trigger_end_us"] != expected_trigger_end:
        raise ProtocolError("resolved run config: trigger_end_us is inconsistent")
    if resolved["run_end_us"] != expected_run_end:
        raise ProtocolError("resolved run config: run_end_us is inconsistent")

    if resolved["run_config_sha256"] != canonical_sha256(resolved):
        raise ProtocolError("resolved run config: run_config_sha256 mismatch")


def write_resolved_run_config(path: Path, resolved: dict[str, Any]) -> None:
    path.write_text(
        json.dumps(
            resolved,
            ensure_ascii=False,
            sort_keys=True,
            indent=2,
            allow_nan=False,
        )
        + "\n",
        encoding="utf-8",
    )
