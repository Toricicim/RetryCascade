"""Derived diagnostic artifacts; calendar features are not cohort outcomes."""

from __future__ import annotations

import statistics
from collections import defaultdict
from pathlib import Path

from retrycascade.pilot import write_json
from retrycascade.recovery_features import (
    derive_recovery_features,
    independent_failure_nulls,
    stationary_budget_reference,
)


def validate_trace(summary: dict, config: dict) -> None:
    """Verify trace accounting, not empirical validity of the model.

    Work/busy totals are exact. Calendar event bins use [start,end), while
    engine horizon totals may include events at run_end itself; bin event
    totals must therefore not exceed engine totals, not necessarily equal.
    """
    trace = summary["v2_metrics"].get("recovery_trace")
    if trace is None:
        return
    windows = trace["windows"]
    if not windows:
        raise ValueError("empty recovery trace")
    expected_start = config["observation"]["arrivals_begin_us"]
    capacity = trace["total_waiting_capacity"]
    for window in windows:
        start, end = window["start_us"], window["end_us"]
        if start != expected_start or end <= start:
            raise ValueError("non-contiguous recovery trace")
        expected_start = end
        if not 0 <= window["queue_area_slot_us"] <= capacity * (end - start):
            raise ValueError("queue area exceeds bounded capacity")
        for busy, available in zip(
            window["stage_busy_worker_us"], window["stage_available_worker_us"]
        ):
            if not 0 <= busy <= available:
                raise ValueError("trace worker occupancy exceeds capacity")
        cohort = window["cohort"]
        if cohort["successes"] + cohort["failures"] != cohort["observed_outcomes"]:
            raise ValueError("trace cohort outcomes do not conserve")
        if cohort["observed_outcomes"] + cohort["right_censored"] != cohort["arrivals"]:
            raise ValueError("trace cohort arrivals do not conserve")
        if (
            window["cohort_deadline_abandoned"]
            + window["cohort_attempts_exhausted"]
            + window["cohort_budget_denied"]
            != cohort["failures"]
        ):
            raise ValueError("trace cohort failure reasons do not conserve")
    if expected_start != config["run_end_us"]:
        raise ValueError("trace does not reach run end")
    for name in ("stage_consumed_work_milli", "stage_busy_worker_us"):
        summed = [sum(w[name][stage] for w in windows) for stage in range(3)]
        if summed != summary["v2_metrics"][name]:
            raise ValueError(f"trace {name} does not conserve")
    for name in (
        "logical_arrivals",
        "unique_goodput",
        "attempt_timeouts",
        "pre_admission_failures",
        "queue_rejections",
        "late_completions",
        "duplicate_completions",
    ):
        if sum(w[name] for w in windows) > summary["counts"].get(name, 0):
            raise ValueError(f"trace {name} count exceeds horizon total")


def analyze_followup(
    output: Path, runs: list[dict], rows: list[dict], plan: dict
) -> list[str]:
    """Write flat feature/null rows plus scientific plots, returning filenames."""
    if not plan.get("followup_protocol"):
        return []
    by_id = {row["run_id"]: row for row in rows}
    features = []
    references = []
    outcomes = []
    for run in runs:
        summary, config, metadata = run["summary"], run["config"], run["metadata"]
        trace = summary["v2_metrics"].get("recovery_trace")
        if trace is None:
            raise ValueError("followup requires recovery trace")
        validate_trace(summary, config)
        run_id = config["run_config_sha256"]
        row = by_id[run_id]
        common = {
            "run_id": run_id,
            "arm": metadata["arm"],
            "seed": metadata["seed"],
            "rho": row["rho"],
            "failure_rate": row["failure_rate"],
            "trigger_seconds": row["trigger_seconds"],
            "reference_trigger_seconds": row["reference_trigger_seconds"],
            "client_contract": row["client_contract"],
        }
        recovery = summary["recovery"]
        baseline = {
            "eligible": row["baseline_eligible"],
            "g0_per_second": recovery["baseline_g0_milli_per_second"] / 1000,
            "q0_slots": recovery["baseline_q0_milli_slots"] / 1000,
            "available_at_us": trace["reference_trigger_start_us"],
        }
        capacity = trace["total_waiting_capacity"]
        recovery_spec = config["recovery"]
        spec = {
            "queue_capacity_waiting": capacity,
            "queue_tolerance_slots": recovery_spec[
                "queue_margin_fraction_of_total_waiting_capacity_basis_points"
            ]
            / 10000
            * capacity,
            "goodput_target_fraction": recovery_spec[
                "goodput_fraction_of_g0_basis_points"
            ]
            / 10000,
            "analysis_start_us": trace["reference_trigger_end_us"],
            "rolling_window_us": recovery_spec["window_us"],
            "ewma_half_life_us": plan["followup_protocol"]["analysis"][
                "ewma_half_life_us"
            ],
        }
        for feature, raw in zip(
            derive_recovery_features(trace["windows"], baseline, spec), trace["windows"]
        ):
            features.append(
                {
                    **common,
                    **feature,
                    "seconds_since_reference_trigger_end": (
                        feature["end_us"] - trace["reference_trigger_end_us"]
                    )
                    / 1e6,
                    "baseline_g0_per_second": baseline["g0_per_second"],
                    "baseline_q0_slots": baseline["q0_slots"],
                    "queue_target_slots": baseline["q0_slots"]
                    + spec["queue_tolerance_slots"],
                    "retry_attempts": raw["retry_attempts"],
                    "attempt_timeouts": raw["attempt_timeouts"],
                    "budget_denied": raw["budget_denied"],
                    "pre_admission_failures": raw["pre_admission_failures"],
                    "calendar_consumed_work_units_per_second": sum(
                        raw["stage_consumed_work_milli"]
                    )
                    / 1000
                    / feature["duration_seconds"],
                    "sink_busy_fraction": raw["stage_busy_worker_us"][2]
                    / raw["stage_available_worker_us"][2]
                    if raw["stage_available_worker_us"][2]
                    else None,
                    "sink_work_capacity_fraction": raw["stage_consumed_work_milli"][2]
                    / raw["stage_capacity_work_milli"][2]
                    if raw["stage_capacity_work_milli"][2]
                    else None,
                }
            )
        for phase in trace["cohorts"]:
            cohort = phase["outcomes"]
            denominator = cohort["observed_outcomes"]
            phase_windows = [
                window
                for window in trace["windows"]
                if window["start_us"] >= phase["start_us"]
                and window["end_us"] <= phase["end_us"]
            ]
            if (
                sum(window["end_us"] - window["start_us"] for window in phase_windows)
                != phase["end_us"] - phase["start_us"]
            ):
                raise ValueError(
                    "calendar bins must split at reference cohort boundaries"
                )
            outcomes.append(
                {
                    **common,
                    "phase": cohort["name"],
                    "arrival_interval_start_us": phase["start_us"],
                    "arrival_interval_end_us": phase["end_us"],
                    "retrospective_available_at_us": phase["mature_available_at_us"],
                    "observed_outcomes": denominator,
                    "right_censored": cohort["right_censored"],
                    "success_ratio": cohort["successes"] / denominator
                    if denominator
                    else None,
                    "cohort_budget_denied_ratio": phase["budget_denied"] / denominator
                    if denominator
                    else None,
                    "calendar_interval_consumed_work_units": sum(
                        sum(window["stage_consumed_work_milli"])
                        for window in phase_windows
                    )
                    / 1000,
                    "calendar_interval_busy_worker_seconds": sum(
                        sum(window["stage_busy_worker_us"]) for window in phase_windows
                    )
                    / 1e6,
                    "cost_semantics": "Calendar-interval physical work, not work attributable to the same arrival cohort",
                    "failure_p95_ms": cohort["failure_latency"]["p95_us"] / 1000
                    if cohort["failure_latency"]["p95_us"] is not None
                    else None,
                }
            )
        if row["trigger_seconds"] == 0:
            p = row["failure_rate"]
            attempts = config["retry_contract"]["max_attempts_including_initial"]
            nulls = independent_failure_nulls(p, attempts)
            budget = metadata["refill_basis_points"]
            if budget is not None:
                bound = stationary_budget_reference(p, budget / 10000, attempts)
                expected = bound["stationary_iid_success_upper_bound"]
                kind = "stationary_iid_success_upper_bound"
                assumptions = bound["assumptions"]
            else:
                expected = nulls["unlimited_budget_max_attempts_success"]
                kind = "iid_no_load_success_reference"
                assumptions = nulls["assumptions"]
            references.append(
                {
                    **common,
                    "reference_kind": kind,
                    "reference_success": expected,
                    "observed_success": row["success_ratio"],
                    "observed_minus_reference": row["success_ratio"] - expected
                    if row["success_ratio"] is not None
                    else None,
                    "interpretation": "Descriptive distance, not a significance test or unexplained effect; queue/deadline/finite-token behavior can violate ideal-reference assumptions",
                    "assumptions": assumptions,
                }
            )
    write_json(output / "recovery_features.json", features)
    write_json(output / "analytical_references.json", references)
    write_json(output / "incident_outcomes.json", outcomes)
    return [
        "recovery_features.json",
        "analytical_references.json",
        "incident_outcomes.json",
        *_plot(output, features, outcomes),
    ]


def _plot(output: Path, features: list[dict], outcomes: list[dict]) -> list[str]:
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    plt.rcParams["svg.hashsalt"] = "retrycascade-followup-v1"
    colors = {
        "budget_10pct": "#0072B2",
        "full_jitter": "#D55E00",
        "no_retry": "#009E73",
    }
    selected = [
        r
        for r in features
        if r["failure_rate"] == 0.1
        and r["reference_trigger_seconds"] == 10
        and r["trigger_seconds"] == 10
        and r["client_contract"] == "strict_rpc"
    ]
    rhos = sorted({r["rho"] for r in selected})
    fig, axes = plt.subplots(
        max(1, len(rhos)), 3, figsize=(15, 3.6 * max(1, len(rhos))), squeeze=False
    )
    columns = [
        ("descriptive_recovery_index", "Descriptive recovery index (0–100)"),
        ("goodput_ratio_to_baseline", "Completed useful work / own baseline"),
        ("mean_waiting_slots", "Exact mean waiting queue (slots)"),
    ]
    for ri, rho in enumerate(rhos):
        for ci, (key, label) in enumerate(columns):
            ax = axes[ri][ci]
            for arm in sorted({r["arm"] for r in selected}):
                grouped = defaultdict(list)
                for row in selected:
                    if row["rho"] == rho and row["arm"] == arm and row[key] is not None:
                        grouped[row["seconds_since_reference_trigger_end"]].append(
                            row[key]
                        )
                xs = sorted(grouped)
                means = [statistics.mean(grouped[x]) for x in xs]
                ax.plot(xs, means, label=arm, color=colors[arm])
                ax.fill_between(
                    xs,
                    [min(grouped[x]) for x in xs],
                    [max(grouped[x]) for x in xs],
                    alpha=0.12,
                    color=colors[arm],
                )
            ax.axvline(0, color="black", linewidth=0.8)
            if ci == 0:
                ax.axhline(100, color="gray", linestyle=":", linewidth=0.8)
                ax.set_ylim(-2, 105)
            elif ci == 1:
                ax.axhline(0.95, color="gray", linestyle=":", linewidth=0.8)
            ax.set_title(f"rho={rho:g} · {label}")
            ax.set_xlabel("Seconds since reference incident end (bin end)")
            ax.grid(alpha=0.2)
            ax.legend(fontsize=8)
    if not rhos:
        axes[0][0].text(
            0.5, 0.5, "Representative p=0.1 / 10s cell not present", ha="center"
        )
    fig.suptitle(
        "Calendar-bin operational features · seed means, shaded seed min–max (not confidence intervals)"
    )
    fig.tight_layout(rect=(0, 0, 1, 0.95))
    names = _save(fig, output, "recovery_curves")
    plt.close(fig)
    phases = [
        "during_reference_trigger",
        "post_reference_first_60s",
        "post_reference_later",
    ]
    chosen = [
        r
        for r in outcomes
        if r["failure_rate"] == 0.1
        and r["reference_trigger_seconds"] == 10
        and r["client_contract"] == "strict_rpc"
    ]
    fig, axes = plt.subplots(
        1, max(1, len(rhos)), figsize=(5.2 * max(1, len(rhos)), 4.8), squeeze=False
    )
    for ri, rho in enumerate(rhos):
        ax = axes[0][ri]
        for arm in sorted({r["arm"] for r in chosen}):
            for triggered in (False, True):
                groups = [
                    [
                        r["success_ratio"]
                        for r in chosen
                        if r["rho"] == rho
                        and r["arm"] == arm
                        and bool(r["trigger_seconds"]) == triggered
                        and r["phase"] == phase
                        and r["success_ratio"] is not None
                    ]
                    for phase in phases
                ]
                ys = [
                    statistics.mean(group) * 100 if group else float("nan")
                    for group in groups
                ]
                ax.plot(
                    range(3),
                    ys,
                    marker="o",
                    linestyle="-" if triggered else "--",
                    label=f"{arm} {'incident' if triggered else 'matched control'}",
                    color=colors[arm],
                )
        ax.set_xticks(
            range(3),
            ["Incident arrivals", "Next 60s arrivals", "Later arrivals"],
            rotation=15,
        )
        ax.set_ylim(-2, 102)
        ax.set_ylabel("Absolute mature-cohort success (%)")
        ax.set_title(f"rho={rho:g}")
        ax.grid(alpha=0.2)
        ax.legend(fontsize=7)
    fig.suptitle(
        "Retrospective arrival-cohort outcomes · not live features · seed means"
    )
    fig.tight_layout(rect=(0, 0, 1, 0.94))
    names.extend(_save(fig, output, "incident_outcomes"))
    plt.close(fig)
    return names


def _save(fig, output: Path, stem: str) -> list[str]:
    fig.savefig(output / f"{stem}.png", dpi=150)
    fig.savefig(output / f"{stem}.svg", metadata={"Date": None})
    return [f"{stem}.png", f"{stem}.svg"]
