"""Causal, descriptive recovery features from completed calendar bins.

These are not learned predictors and do not replace Rust's recovery decision.
All rates/EMA/AUC values are calculated from bin aggregates, not individual
event timestamps. Arrival-cohort outcomes are deliberately never read here:
they become known later and would leak future outcomes into live features.
"""

from __future__ import annotations

import math
from collections import deque
from typing import Any

FEATURE_VERSION = "recovery-features-v1"


def _number(value: Any, name: str, *, positive: bool = False) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise TypeError(f"{name} must be numeric")
    result = float(value)
    if not math.isfinite(result) or result < 0 or (positive and result == 0):
        raise ValueError(
            f"{name} must be finite and {'positive' if positive else 'nonnegative'}"
        )
    return result


def _tick(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{name} must be a nonnegative integer")
    return value


def _clip(value: float) -> float:
    return min(1.0, max(0.0, value))


def derive_recovery_features(
    series: list[dict], baseline: dict, spec: dict
) -> list[dict]:
    """Return one row at each bin end, without consulting later bins.

    Required baseline keys: ``eligible``, ``g0_per_second``, ``q0_slots``.
    Optional ``available_at_us`` prevents using a pre-trigger reference before
    its observation period ends. Zero/missing/ineligible G0 makes normalized
    features null, never zero. Required spec: ``queue_capacity_waiting`` and
    ``queue_tolerance_slots`` (caller derives margin * capacity from protocol).
    Optional spec: goodput_target_fraction=.95, rolling_window_us=30_000_000,
    ewma_half_life_us=10_000_000, analysis_start_us=first bin start.

    The display index is 100 * min(Ghealth, Qhealth), with
    Ghealth=clip(G/(target*G0)) and
    Qhealth=clip(1-max(0,Q-Qlimit)/(K-Qlimit)), Qlimit=Q0+tolerance.
    If Qlimit>=K every physically valid bounded queue passes Qhealth=1.
    Thus 100 means both descriptive bin targets pass, not sustained recovery
    or restoration of user success. Overshoot stays visible in raw G/G0.

    Rolling fields require exact whole-bin coverage of the trailing duration;
    split calendar bins may be combined, but no bin is silently interpolated.
    EMA uses completed bin
    means and elapsed bin duration. Delta rates use adjacent bin-end times.
    AUC integrates each bin mean over its overlap with analysis_start_us.
    """
    capacity = _number(
        spec["queue_capacity_waiting"], "queue_capacity_waiting", positive=True
    )
    tolerance = _number(spec["queue_tolerance_slots"], "queue_tolerance_slots")
    target = _number(
        spec.get("goodput_target_fraction", 0.95),
        "goodput_target_fraction",
        positive=True,
    )
    if target > 1:
        raise ValueError("goodput_target_fraction must be <= 1")
    rolling_us = _tick(spec.get("rolling_window_us", 30_000_000), "rolling_window_us")
    half_life_us = _tick(spec.get("ewma_half_life_us", 10_000_000), "ewma_half_life_us")
    if not rolling_us or not half_life_us:
        raise ValueError("rolling window and half life must be positive")
    available_at = _tick(baseline.get("available_at_us", 0), "available_at_us")
    analysis_start = _tick(
        spec.get("analysis_start_us", series[0]["start_us"] if series else 0),
        "analysis_start_us",
    )
    g0 = baseline.get("g0_per_second")
    q0 = baseline.get("q0_slots")
    if g0 is not None:
        g0 = _number(g0, "g0_per_second")
    if q0 is not None:
        q0 = _number(q0, "q0_slots")
        if q0 > capacity:
            raise ValueError("baseline queue exceeds waiting capacity")
    reference_valid = (
        baseline.get("eligible") is True
        and g0 is not None
        and g0 > 0
        and q0 is not None
    )
    rows: list[dict] = []
    trailing: deque[dict] = deque()
    previous_end: int | None = None
    previous_goodput: float | None = None
    previous_queue: float | None = None
    ema_goodput: float | None = None
    ema_queue: float | None = None
    deficit_auc = 0.0
    queue_auc = 0.0

    def normalized(goodput: float, queue: float) -> tuple[float, float, float, float]:
        assert g0 is not None and q0 is not None
        ratio = goodput / g0
        excess = max(0.0, queue - q0)
        limit = q0 + tolerance
        health = (
            1.0
            if limit >= capacity
            else _clip(1.0 - max(0.0, queue - limit) / (capacity - limit))
        )
        return (
            ratio,
            excess / capacity,
            health,
            100.0 * min(_clip(ratio / target), health),
        )

    for source in series:
        start = _tick(source["start_us"], "start_us")
        end = _tick(source["end_us"], "end_us")
        if end <= start or (previous_end is not None and start != previous_end):
            raise ValueError(
                "series must contain positive-duration contiguous chronological bins"
            )
        duration = end - start
        seconds = duration / 1e6
        count = _tick(source["unique_goodput"], "unique_goodput")
        area = _number(source["queue_area_slot_us"], "queue_area_slot_us")
        queue = area / duration
        if queue > capacity + 1e-9:
            raise ValueError("queue area exceeds bounded waiting capacity")
        goodput = count / seconds
        alpha = -math.expm1(-math.log(2) * duration / half_life_us)
        ema_goodput = (
            goodput
            if ema_goodput is None
            else ema_goodput + alpha * (goodput - ema_goodput)
        )
        ema_queue = (
            queue if ema_queue is None else ema_queue + alpha * (queue - ema_queue)
        )
        row = {
            "feature_version": FEATURE_VERSION,
            "start_us": start,
            "end_us": end,
            "available_at_us": end,
            "duration_seconds": seconds,
            "event_goodput_per_second": goodput,
            "mean_waiting_slots": queue,
            "queue_headroom_fraction": _clip(1.0 - queue / capacity),
            "bin_ewma_goodput_per_second": ema_goodput,
            "bin_ewma_waiting_slots": ema_queue,
            "goodput_delta_per_second_squared": None
            if previous_goodput is None
            else (goodput - previous_goodput) / ((end - previous_end) / 1e6),
            "queue_delta_slots_per_second": None
            if previous_queue is None
            else (queue - previous_queue) / ((end - previous_end) / 1e6),
            "baseline_usable": reference_valid and end >= available_at,
            "goodput_ratio_to_baseline": None,
            "queue_excess_fraction_of_capacity": None,
            "queue_health_fraction": None,
            "descriptive_recovery_index": None,
            "goodput_deficit_auc_ratio_seconds": None,
            "queue_excess_auc_capacity_seconds": None,
            "rolling_goodput_per_second": None,
            "rolling_mean_waiting_slots": None,
            "rolling_descriptive_recovery_index": None,
            "rolling_targets_pass": None,
        }
        if row["baseline_usable"]:
            ratio, excess, health, index = normalized(goodput, queue)
            observed_seconds = (
                max(0, end - max(start, analysis_start, available_at)) / 1e6
            )
            deficit_auc += max(0.0, 1.0 - ratio) * observed_seconds
            queue_auc += excess * observed_seconds
            row.update(
                goodput_ratio_to_baseline=ratio,
                queue_excess_fraction_of_capacity=excess,
                queue_health_fraction=health,
                descriptive_recovery_index=index,
                goodput_deficit_auc_ratio_seconds=deficit_auc,
                queue_excess_auc_capacity_seconds=queue_auc,
            )
        trailing.append(source)
        cutoff = end - rolling_us
        while trailing and trailing[0]["end_us"] <= cutoff:
            trailing.popleft()
        complete = trailing[0]["start_us"] == cutoff
        if complete:
            rolling_goodput = sum(item["unique_goodput"] for item in trailing) / (
                rolling_us / 1e6
            )
            rolling_queue = (
                sum(item["queue_area_slot_us"] for item in trailing) / rolling_us
            )
            row.update(
                rolling_goodput_per_second=rolling_goodput,
                rolling_mean_waiting_slots=rolling_queue,
            )
            if row["baseline_usable"] and cutoff >= available_at:
                row["rolling_descriptive_recovery_index"] = normalized(
                    rolling_goodput, rolling_queue
                )[3]
                row["rolling_targets_pass"] = (
                    rolling_goodput >= target * g0 and rolling_queue <= q0 + tolerance
                )
        rows.append(row)
        previous_end, previous_goodput, previous_queue = end, goodput, queue
    return rows


def independent_failure_nulls(probability: float, max_attempts: int = 3) -> dict:
    """Elementary no-load IID references, not predictions for queued systems."""
    probability = _number(probability, "probability")
    if probability > 1:
        raise ValueError("probability must be <= 1")
    max_attempts = _tick(max_attempts, "max_attempts")
    if not max_attempts:
        raise ValueError("max_attempts must be positive")
    return {
        "no_retry_success": 1.0 - probability,
        "unlimited_budget_max_attempts_success": 1.0 - probability**max_attempts,
        "expected_attempts_without_deadlines": sum(
            probability**attempt for attempt in range(max_attempts)
        ),
        "assumptions": "IID pre-admission failure; no queue loss, timeout, deadline, or binding retry budget; each retry permitted until max_attempts",
    }


def stationary_budget_reference(
    probability: float, refill_fraction: float, max_attempts: int = 3
) -> dict:
    """Stationary IID token-flow upper bound, not an exact bucket prediction.

    With r permitted retries per arrival and s successes per arrival,
    s=(1-p)*(1+r) and r<=beta*s once initial tokens are negligible.
    Combining this with the max-attempt success bound yields the expression
    below. Unused/capped tokens can make actual success smaller. Queueing,
    timeout/late work, finite horizons and initial tokens invalidate equality.
    """
    nulls = independent_failure_nulls(probability, max_attempts)
    probability = float(probability)
    refill_fraction = _number(refill_fraction, "refill_fraction")
    denominator = 1.0 - refill_fraction * (1.0 - probability)
    unconstrained = nulls["unlimited_budget_max_attempts_success"]
    token_bound = (1.0 - probability) / denominator if denominator > 0 else 1.0
    success_bound = min(unconstrained, token_bound)
    return {
        "stationary_iid_success_upper_bound": success_bound,
        "unlimited_budget_max_attempts_success": unconstrained,
        "token_constraint_binding_in_mean_field": token_bound < unconstrained,
        "assumptions": "Stationary IID pre-admission failures; retry tokens refilled only by unique timely success; initial tokens negligible; no queue loss, timeout, or deadline; finite bucket saturation and unused tokens can reduce success; not an exact finite-run prediction",
    }
