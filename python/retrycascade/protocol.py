"""Research-protocol loading, semantic validation, and canonical hashing."""

from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
from typing import Any

import jsonschema
import yaml


class ProtocolError(ValueError):
    """Raised when a protocol is structurally or semantically invalid."""


class _UniqueKeyLoader(yaml.SafeLoader):
    pass


def _construct_unique_mapping(
    loader: yaml.SafeLoader, node: yaml.MappingNode, deep: bool = False
) -> dict[str, Any]:
    mapping: dict[str, Any] = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in mapping:
            raise ProtocolError(f"Duplicate YAML key: {key!r}")
        mapping[key] = loader.construct_object(value_node, deep=deep)
    return mapping


_UniqueKeyLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG,
    _construct_unique_mapping,
)


def _load_yaml(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        loaded = yaml.load(handle, Loader=_UniqueKeyLoader)
    if not isinstance(loaded, dict):
        raise ProtocolError("Top-level YAML value must be an object")
    return loaded


def _load_schema(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def _require_integer_domain(value: Any, path: str = "$") -> None:
    if isinstance(value, float):
        raise ProtocolError(
            f"Floating-point value is forbidden in canonical protocol: {path}"
        )
    if isinstance(value, dict):
        for key, child in value.items():
            _require_integer_domain(child, f"{path}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _require_integer_domain(child, f"{path}[{index}]")


def _strictly_increasing(values: list[int], name: str) -> None:
    if values != sorted(set(values)):
        raise ProtocolError(f"{name} must contain unique, strictly increasing values")


def validate_protocol_semantics(protocol: dict[str, Any]) -> None:
    _require_integer_domain(protocol)

    stages = protocol["topology"]["stages"]
    if [stage["name"] for stage in stages] != ["source", "transform", "sink"]:
        raise ProtocolError(
            "Topology must contain source, transform, sink in that order"
        )
    rates = [stage["nominal_rate_work_units_per_second"] for stage in stages]
    if any(rate <= 0 for rate in rates) or rates[-1] != min(rates):
        raise ProtocolError("Sink must be the positive nominal bottleneck")
    if any(
        stage["workers"] < 1 or stage["queue_capacity_waiting"] < 1 for stage in stages
    ):
        raise ProtocolError("Workers and waiting queue capacities must be positive")

    rho = protocol["workload"]["arrival_process"]["rho_basis_points"]
    _strictly_increasing(rho, "rho_basis_points")
    if any(value <= 0 or value >= 10_000 for value in rho):
        raise ProtocolError("rho_basis_points values must satisfy 0 < rho < 10000")

    retry = protocol["retry_contract"]
    if retry["per_attempt_timeout_us"] <= 0:
        raise ProtocolError("per-attempt timeout must be positive")
    if retry["per_attempt_timeout_us"] >= retry["overall_logical_deadline_us"]:
        raise ProtocolError("per-attempt timeout must be less than overall deadline")
    if retry["max_attempts_including_initial"] < 1:
        raise ProtocolError("max_attempts_including_initial must be positive")

    allowed_policies = {
        "immediate",
        "exponential",
        "full_jitter",
        "retry_budget",
    }
    enabled_policies = protocol["policies"]["enabled"]

    if set(enabled_policies) != allowed_policies:
        raise ProtocolError("enabled policies contain an unknown or missing policy")

    common = protocol["policies"]["common"]
    if not 0 < common["exponential_base_us"] <= common["exponential_cap_us"]:
        raise ProtocolError("exponential base must be positive and no greater than cap")
    if protocol["policies"]["immediate"]["epsilon_us"] < 1:
        raise ProtocolError("immediate epsilon must be at least one tick")
    budget = protocol["policies"]["retry_budget"]["token_bucket_fixed_point"]
    if not 0 < budget["initial_units"] <= budget["capacity_units"]:
        raise ProtocolError("retry budget initial units must be within capacity")
    if (
        budget["retry_cost_units"] < 1
        or budget["refill_units_per_unique_on_time_success"] < 1
    ):
        raise ProtocolError("retry budget cost and refill must be positive")
    if protocol["policies"]["retry_budget"]["source_load_shedding"]:
        raise ProtocolError(
            "source shedding must remain outside the primary retry-budget policy"
        )

    rng = protocol["rng"]
    if rng["arrival_key"] != ["main_seed", "logical_arrival_index"]:
        raise ProtocolError("arrival RNG key does not match the frozen contract")
    if rng["service_key"] != [
        "main_seed",
        "logical_request_id",
        "attempt_no",
        "stage",
    ]:
        raise ProtocolError("service RNG key does not match the frozen contract")
    if rng["jitter_key"] != [
        "main_seed",
        "logical_request_id",
        "attempt_no",
    ]:
        raise ProtocolError(
            "jitter RNG key must be shared by full-jitter and retry-budget"
        )

    trigger = protocol["trigger"]
    _strictly_increasing(trigger["durations_us"], "trigger.durations_us")
    if trigger["start_us"] <= 0 or any(
        duration <= 0 for duration in trigger["durations_us"]
    ):
        raise ProtocolError("trigger start and durations must be positive")
    if not 0 < trigger["capacity_multiplier_basis_points"] <= 10_000:
        raise ProtocolError("trigger capacity multiplier must be in (0, 10000]")

    observation = protocol["observation"]
    if not (
        observation["arrivals_begin_us"]
        <= observation["warmup_end_us"]
        <= observation["baseline_start_us"]
        < observation["baseline_end_us"]
        == trigger["start_us"]
    ):
        raise ProtocolError("warmup, baseline, and trigger start ordering is invalid")
    sensitivity = observation["sensitivity_horizons_us"]
    _strictly_increasing(sensitivity, "sensitivity_horizons_us")
    if observation["primary_recovery_horizon_us"] not in sensitivity:
        raise ProtocolError("primary recovery horizon must be in sensitivity horizons")
    if observation["post_trigger_artifact_horizon_us"] < max(sensitivity):
        raise ProtocolError("artifact horizon must cover all sensitivity horizons")

    baseline = protocol["baseline"]
    baseline_duration = (
        observation["baseline_end_us"] - observation["baseline_start_us"]
    )
    if (
        baseline_duration < 4 * baseline["window_us"]
        or baseline_duration % baseline["window_us"]
    ):
        raise ProtocolError("baseline must contain at least four whole windows")
    if baseline["window_us"] % observation["queue_sample_interval_us"]:
        raise ProtocolError("queue sample interval must divide the baseline window")

    recovery = protocol["recovery"]
    required_recovery_span = (
        recovery["window_us"] * recovery["consecutive_non_overlapping_windows"]
    )
    if required_recovery_span > observation["primary_recovery_horizon_us"]:
        raise ProtocolError("recovery windows do not fit in primary horizon")

    seeds = protocol["experiment"]["main_seeds"]
    if len(seeds) != len(set(seeds)) or not all(
        0 <= seed <= 2**53 - 1 for seed in seeds
    ):
        raise ProtocolError("main seeds must be unique JSON-safe non-negative integers")
    expected_trigger = (
        len(rho)
        * len(trigger["durations_us"])
        * len(protocol["policies"]["enabled"])
        * len(seeds)
    )
    expected_control = len(rho) * len(protocol["policies"]["enabled"]) * len(seeds)
    experiment = protocol["experiment"]
    if expected_trigger != experiment["expected_trigger_runs"]:
        raise ProtocolError("expected_trigger_runs does not match the factor matrix")
    if expected_control != experiment["expected_no_trigger_runs"]:
        raise ProtocolError("expected_no_trigger_runs does not match the factor matrix")
    if expected_trigger + expected_control != experiment["expected_total_primary_runs"]:
        raise ProtocolError(
            "expected_total_primary_runs does not match trigger + control runs"
        )

    if protocol["safety"]["maximum_events_per_run"] < 1:
        raise ProtocolError("maximum_events_per_run must be positive")


def validate_run_config_semantics(config: dict[str, Any]) -> None:
    _require_integer_domain(config)

    trigger_enabled = config["trigger_enabled"]
    trigger_duration = config["trigger_duration_us"]

    if trigger_enabled and trigger_duration <= 0:
        raise ProtocolError("enabled trigger must have a positive duration")

    if not trigger_enabled and trigger_duration != 0:
        raise ProtocolError("disabled trigger must have zero duration")

    if config.get("per_attempt_timeout_us", 1) >= config.get(
        "overall_logical_deadline_us", 2
    ):
        raise ProtocolError("per-attempt timeout must be less than overall deadline")
    if "trigger_start_us" in config and "post_trigger_horizon_us" in config:
        end = config["trigger_start_us"] + config["trigger_duration_us"]
        if end > 2**64 - 1 or config["post_trigger_horizon_us"] < 1:
            raise ProtocolError("invalid trigger or horizon arithmetic")


def load_and_validate(path: Path, schema_path: Path, *, kind: str) -> dict[str, Any]:
    document = _load_yaml(path)
    try:
        jsonschema.Draft202012Validator(_load_schema(schema_path)).validate(document)
    except jsonschema.ValidationError as exc:
        raise ProtocolError(exc.message) from exc
    if kind == "protocol":
        validate_protocol_semantics(document)
    elif kind == "run":
        validate_run_config_semantics(document)
    else:
        raise ValueError(f"Unknown validation kind: {kind}")
    return document


def canonical_bytes(document: dict[str, Any]) -> bytes:
    normalized = copy.deepcopy(document)
    for excluded in ("protocol_sha256", "run_config_sha256"):
        normalized.pop(excluded, None)
    _require_integer_domain(normalized)
    return json.dumps(
        normalized,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    ).encode("utf-8")


def canonical_sha256(document: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_bytes(document)).hexdigest()
