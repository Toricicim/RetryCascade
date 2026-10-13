"""RetryCascade V2: plan, run/resume, Parquet + DuckDB analysis and HTML report.

python -m retrycascade.study run --preset pilot --output ../output/v2-pilot
python -m retrycascade.study run --preset pilot --output ../output/v2-pilot
python -m retrycascade.study analyze --output ../output/v2-pilot
python -m retrycascade.study query --output ../output/v2-pilot --sql 'SELECT * FROM cells'
"""

from __future__ import annotations

import argparse
import fcntl
import html
import json
import math
import random
import shutil
import statistics
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import contextmanager
from pathlib import Path

import duckdb

from retrycascade.pilot import (
    digest_file,
    ratio,
    recovered,
    validate_summary,
    write_json,
)
from retrycascade.protocol import _load_yaml, canonical_sha256
from retrycascade.v2_config import ROOT, load_v2_protocol, resolve_v2

LOADED_SOURCE_SHA256 = digest_file(Path(__file__))


@contextmanager
def study_lock(output: Path, *, shared: bool = False):
    """macOS/Linux kernel lock: released automatically even after a crash."""
    output.mkdir(parents=True, exist_ok=True)
    with (output / ".study.lock").open("a") as lock:
        mode = fcntl.LOCK_SH if shared else fcntl.LOCK_EX
        try:
            fcntl.flock(lock, mode | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ValueError("another process is using this study directory") from error
        try:
            yield
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)


def make_plan(preset: str) -> dict:
    v2, base = load_v2_protocol()
    if preset in ("followup", "followup-smoke"):
        return make_followup_plan(preset, v2, base)
    factors = v2["discovery"]
    seeds = factors["seeds"]
    rhos = factors["rho_basis_points"]
    probabilities = v2["failure"]["probabilities_basis_points"]
    durations = factors["trigger_durations_us"]
    if preset == "smoke":
        seeds, rhos, probabilities, durations = (
            seeds[:1],
            [5000],
            [0, 2000],
            [0, 10000000],
        )
    elif preset == "pilot":
        seeds, durations = seeds[:3], [0, 10000000]
    elif preset != "discovery":
        raise ValueError(f"unknown preset {preset}")
    jobs = {}

    def add(
        block: str,
        rho: int,
        probability: int,
        duration: int,
        seed: int,
        policy: str,
        refill_bp: int = 1000,
        timeout_us: int | None = None,
        identity: str | None = None,
    ) -> None:
        config = resolve_v2(
            v2,
            base,
            rho=rho,
            probability=probability,
            duration=duration,
            policy=policy,
            seed=seed,
            refill_bp=refill_bp,
            timeout_us=timeout_us,
            demand_identity=identity,
        )
        arm = f"budget_{refill_bp / 100:g}pct" if policy == "retry_budget" else policy
        metadata = {
            "block": block,
            "phase": "exploratory_discovery",
            "arm": arm,
            "policy": policy,
            "rho_basis_points": rho,
            "failure_basis_points": probability,
            "duration_us": duration,
            "refill_basis_points": refill_bp if policy == "retry_budget" else None,
            "timeout_us": config["retry_contract"]["per_attempt_timeout_us"],
            "demand_identity": config["extensions"]["demand_identity"],
            "seed": seed,
        }
        jobs[config["run_config_sha256"]] = {"metadata": metadata, "config": config}

    def arms(
        block: str,
        rho: int,
        p: int,
        duration: int,
        seed: int,
        timeout: int | None = None,
        identity: str | None = None,
    ) -> None:
        for policy in factors["policies"]:
            refills = (
                v2["retry"]["refill_basis_points"]
                if policy == "retry_budget"
                else [1000]
            )
            for refill in refills:
                add(block, rho, p, duration, seed, policy, refill, timeout, identity)

    for rho in rhos:
        for p in probabilities:
            for duration in durations:
                for seed in seeds:
                    arms("main", rho, p, duration, seed)
    if preset == "discovery":
        # Small predeclared sensitivity blocks; do not multiply the whole grid.
        for seed in seeds[: factors["sensitivity_seed_count"]]:
            for duration in durations:
                for timeout in factors["timeout_sensitivity_us"]:
                    arms(
                        f"timeout_{timeout}",
                        factors["sensitivity_rho_basis_points"],
                        factors["sensitivity_failure_basis_points"],
                        duration,
                        seed,
                        timeout,
                    )
                arms(
                    "independent_demand",
                    factors["sensitivity_rho_basis_points"],
                    factors["sensitivity_failure_basis_points"],
                    duration,
                    seed,
                    identity=factors["sensitivity_demand_identity"],
                )
    return {
        "plan_version": "v2.0.0",
        "preset": preset,
        "protocol_sha256": canonical_sha256(v2),
        "seeds": seeds,
        "confirmation_seeds_used": False,
        "jobs": list(jobs.values()),
    }


def make_followup_plan(preset: str, v2: dict, base: dict) -> dict:
    """Freeze a diagnostic study, including calendar-aligned controls."""
    spec = _load_yaml(ROOT / "configs/research_followup.yaml")
    if spec["study_version"] != "retrycascade-followup-v1.0.0":
        raise ValueError("unsupported followup study")
    seeds = spec["seeds"] if preset == "followup" else spec["seeds"][:1]
    if not set(seeds).issubset(v2["discovery"]["seeds"][3:]):
        raise ValueError("followup must use the reserved discovery seeds")
    rhos = spec["rho_basis_points"] if preset == "followup" else [6500]
    probabilities = spec["failure_basis_points"] if preset == "followup" else [1000]
    durations = (
        spec["reference_trigger_durations_us"] if preset == "followup" else [10000000]
    )
    jobs = {}

    def add(rho, p, reference, seed, active, contract):
        for policy in spec["policies"]:
            config = resolve_v2(
                v2,
                base,
                rho=rho,
                probability=p,
                duration=reference if active else 0,
                policy=policy,
                seed=seed,
                refill_bp=spec["refill_basis_points"],
                reference_duration_us=reference,
                accept_timed_out_completion=contract
                in ("accept_until_deadline", "accept_and_await"),
                budget_denial_finishes=contract
                not in ("await_existing", "accept_and_await"),
            )
            arm = "budget_10pct" if policy == "retry_budget" else policy
            metadata = {
                "block": "main",
                "phase": "exploratory_diagnostic",
                "arm": arm,
                "policy": policy,
                "rho_basis_points": rho,
                "failure_basis_points": p,
                "duration_us": reference if active else 0,
                "reference_duration_us": reference,
                "client_contract": contract,
                "refill_basis_points": spec["refill_basis_points"]
                if policy == "retry_budget"
                else None,
                "timeout_us": config["retry_contract"]["per_attempt_timeout_us"],
                "demand_identity": config["extensions"]["demand_identity"],
                "seed": seed,
            }
            jobs[config["run_config_sha256"]] = {"metadata": metadata, "config": config}

    for rho in rhos:
        for p in probabilities:
            for reference in durations:
                for seed in seeds:
                    for active in (False, True):
                        add(rho, p, reference, seed, active, "strict_rpc")
    sensitivity = spec["client_contract_sensitivity"]
    for seed in seeds:
        for contract in sensitivity["variants"]:
            for active in (False, True):
                add(
                    sensitivity["rho_basis_points"],
                    sensitivity["failure_basis_points"],
                    sensitivity["reference_trigger_duration_us"],
                    seed,
                    active,
                    contract,
                )
    return {
        "plan_version": "v2-followup-1.0.0",
        "preset": preset,
        "protocol_sha256": canonical_sha256(v2),
        "seeds": seeds,
        "followup_protocol": spec,
        "followup_protocol_sha256": canonical_sha256(spec),
        "confirmation_seeds_used": False,
        "jobs": list(jobs.values()),
    }


def freeze_plan(output: Path, plan: dict) -> None:
    path = output / "plan.json"
    if path.exists() and json.loads(path.read_text()) != plan:
        raise ValueError("existing plan differs; use a new output directory")
    write_json(path, plan)


def verify_artifact(directory: Path, config: dict, binary_sha256: str) -> dict:
    manifest = json.loads((directory / "manifest.json").read_text())
    if manifest["status"] != "complete" or manifest["binary_sha256"] != binary_sha256:
        raise ValueError("run is incomplete or belongs to another binary")
    for name, expected in manifest["files"].items():
        if digest_file(directory / name) != expected:
            raise ValueError(f"artifact checksum mismatch: {name}")
    if json.loads((directory / "resolved_run_config.json").read_text()) != config:
        raise ValueError("stored config differs from plan")
    summary = json.loads((directory / "run_summary.json").read_text())
    validate_summary(summary, config)
    return summary


def execute_job(
    job: dict, binary: Path, output: Path, binary_hash: str, retry_failed: bool = False
) -> dict:
    config = job["config"]
    directory = output / "runs" / config["run_config_sha256"]
    directory.mkdir(parents=True, exist_ok=True)
    if (directory / "manifest.json").exists():
        # Completed-but-corrupt artifacts fail loudly, never silently replace.
        return {
            **job,
            "summary": verify_artifact(directory, config, binary_hash),
            "resumed": True,
        }
    if (directory / "failure.json").exists() and not retry_failed:
        raise ValueError(
            "previous failure recorded; use --retry-failed after checking failure.json"
        )
    write_json(directory / "resolved_run_config.json", config)
    write_json(directory / "selection.json", job["metadata"])
    started = time.monotonic()
    try:
        result = subprocess.run(
            [str(binary), str(directory / "resolved_run_config.json")],
            check=False,
            capture_output=True,
            text=True,
            timeout=180,
        )
        if result.returncode:
            raise RuntimeError(result.stderr.strip() or f"exit {result.returncode}")
        summary = json.loads(result.stdout)
        validate_summary(summary, config)
        write_json(directory / "run_summary.json", summary)
        write_json(
            directory / "manifest.json",
            {
                "status": "complete",
                "binary_sha256": binary_hash,
                "config_sha256": config["run_config_sha256"],
                "elapsed_seconds": time.monotonic() - started,
                "files": {
                    name: digest_file(directory / name)
                    for name in (
                        "resolved_run_config.json",
                        "selection.json",
                        "run_summary.json",
                    )
                },
            },
        )
        if (directory / "failure.json").exists():
            # Retain historical failure record; mark its successful resolution.
            write_json(
                directory / "failure_resolution.json",
                {"status": "retried_successfully"},
            )
        return {**job, "summary": summary, "resumed": False}
    except Exception as error:
        write_json(
            directory / "failure.json",
            {
                "status": "failed",
                "error": str(error),
                "binary_sha256": binary_hash,
                "config_sha256": config["run_config_sha256"],
            },
        )
        raise


def baseline(run: dict, protocol: dict) -> dict:
    windows = run["summary"]["diagnostics"]["baseline_windows"]
    rates = [
        w["unique_goodput"] / ((w["end_us"] - w["start_us"]) / 1e6) for w in windows
    ]
    queues = [w["queue_area_slot_us"] / (w["end_us"] - w["start_us"]) for w in windows]
    xs = [(w["start_us"] + w["end_us"]) / 2e6 for w in windows]
    mean = statistics.mean(rates)
    cv = statistics.pstdev(rates) / mean if mean else math.inf
    mx, my = statistics.mean(xs), statistics.mean(queues)
    slope = sum((x - mx) * (y - my) for x, y in zip(xs, queues)) / sum(
        (x - mx) ** 2 for x in xs
    )
    rules = protocol["baseline"]
    eligible = (
        len(windows) >= rules["window_count"]
        and all(
            w["logical_arrivals"] >= rules["minimum_arrivals_per_window"]
            for w in windows
        )
        and cv <= rules["maximum_goodput_cv_basis_points"] / 1e4
        and abs(slope)
        <= rules["maximum_absolute_queue_slope_milli_slots_per_second"] / 1e3
        and run["summary"]["recovery"]["baseline_reference_available"]
    )
    return {
        "baseline_eligible": eligible,
        "baseline_goodput_cv": cv if math.isfinite(cv) else None,
        "baseline_queue_slope": slope,
    }


def load_runs(output: Path) -> tuple[dict, dict, list[dict]]:
    plan = json.loads((output / "plan.json").read_text())
    execution = json.loads((output / "execution.json").read_text())
    runs = []
    for job in plan["jobs"]:
        summary = verify_artifact(
            output / "runs" / job["config"]["run_config_sha256"],
            job["config"],
            execution["binary_sha256"],
        )
        runs.append({**job, "summary": summary})
    return plan, execution, runs


def run_key(run: dict) -> tuple:
    m = run["metadata"]
    return (
        m.get("client_contract", "strict_rpc"),
        m.get("reference_duration_us", 0),
        m["rho_basis_points"],
        m["failure_basis_points"],
        m["timeout_us"],
        m["demand_identity"],
        m["arm"],
        m["seed"],
        m["duration_us"],
    )


def verify_analysis(output: Path) -> None:
    manifest = json.loads((output / "analysis_manifest.json").read_text())
    if manifest["status"] != "complete":
        raise ValueError("analysis is incomplete")
    if manifest["plan_sha256"] != digest_file(output / "plan.json"):
        raise ValueError("analysis belongs to a different plan")
    for name, expected in manifest["files"].items():
        if digest_file(output / name) != expected:
            raise ValueError(f"derived artifact checksum mismatch: {name}")


def audit_controls(runs: list[dict]) -> None:
    index = {run_key(r): r for r in runs}
    traces = {}
    for run in runs:
        m, s = run["metadata"], run["summary"]
        if m["duration_us"]:
            key = (*run_key(run)[:-1], 0)
            reference = index[key]["summary"]["diagnostics"]["baseline_windows"]
            if reference != s["diagnostics"]["baseline_windows"]:
                raise ValueError("pre-trigger window/control mismatch")
            if m.get("reference_duration_us") is not None:
                control = index[key]
                if control["config"]["run_end_us"] != run["config"]["run_end_us"]:
                    raise ValueError("calendar matched control horizon mismatch")
                for name in ("arrival_trace_sha256", "potential_service_trace_sha256"):
                    if control["summary"][name] != s[name]:
                        raise ValueError(
                            "calendar matched control exogenous trace mismatch"
                        )
                if (
                    control["summary"]["v2_metrics"]["potential_failure_trace_sha256"]
                    != s["v2_metrics"]["potential_failure_trace_sha256"]
                ):
                    raise ValueError("calendar matched failure trace mismatch")
        pair_key = (
            m.get("reference_duration_us", 0),
            m["rho_basis_points"],
            m["failure_basis_points"],
            m["duration_us"],
            m["timeout_us"],
            m["demand_identity"],
            m["seed"],
        )
        fingerprint = (
            s["arrival_trace_sha256"],
            s["potential_service_trace_sha256"],
            s["v2_metrics"]["potential_failure_trace_sha256"],
        )
        if pair_key in traces and traces[pair_key] != fingerprint:
            raise ValueError(
                "common arrival/service/failure traces differ between arms"
            )
        traces[pair_key] = fingerprint


def run_row(run: dict, protocol: dict) -> dict:
    m, s, config = run["metadata"], run["summary"], run["config"]
    v2 = s["v2_metrics"]
    cohort = next(c for c in v2["outcome_cohorts"] if c["name"] == "all")
    during = next(c for c in v2["outcome_cohorts"] if c["name"] == "during_trigger")
    after = next(c for c in v2["outcome_cohorts"] if c["name"] == "post_trigger")
    c = s["counts"]
    trace = v2.get("recovery_trace")
    aligned = (
        {item["outcomes"]["name"]: item for item in trace["cohorts"]} if trace else {}
    )
    incident = aligned.get("during_reference_trigger", {}).get("outcomes")
    early = aligned.get("post_reference_first_60s", {}).get("outcomes")

    def cohort_ratio(value):
        return ratio(value["successes"], value["observed_outcomes"]) if value else None

    return {
        **m,
        "client_contract": m.get("client_contract", "strict_rpc"),
        "reference_trigger_seconds": m.get("reference_duration_us", m["duration_us"])
        / 1e6,
        **baseline(run, protocol),
        "run_id": config["run_config_sha256"],
        "rho": m["rho_basis_points"] / 1e4,
        "failure_rate": m["failure_basis_points"] / 1e4,
        "trigger_seconds": m["duration_us"] / 1e6,
        "recovered_600": recovered(run),
        "recovered_300": recovered(run, 300000000),
        "recovered_900": recovered(run, 900000000),
        "recovery_seconds": (
            (s["recovery"]["recovery_at_us"] - s["trigger_end_us"]) / 1e6
            if s["recovery"]["recovery_at_us"] is not None
            else None
        ),
        "logical_arrivals": c["logical_arrivals"],
        "physical_attempts": c["physical_attempts"],
        "source_admissions": s["stages"][0]["admissions"],
        "source_admissions_per_physical_attempt": ratio(
            s["stages"][0]["admissions"], c["physical_attempts"]
        ),
        "unique_goodput": c["unique_goodput"],
        "pre_admission_failures": c.get("pre_admission_failures", 0),
        "logical_budget_denied": c.get("logical_budget_denied", 0),
        "queue_rejections": c["queue_rejections"],
        "attempt_timeouts": c["attempt_timeouts"],
        "late_completions": c["late_completions"],
        "duplicate_completions": c["duplicate_completions"],
        "mature_arrivals": cohort["observed_outcomes"],
        "right_censored": cohort["right_censored"],
        "success_ratio": ratio(cohort["successes"], cohort["observed_outcomes"]),
        "during_success_ratio": ratio(during["successes"], during["observed_outcomes"]),
        "during_reference_success_ratio": cohort_ratio(incident),
        "early_post_success_ratio": cohort_ratio(early),
        "matched_incident_excess_failure_ratio": None,
        "matched_early_post_excess_failure_ratio": None,
        "post_success_ratio": ratio(after["successes"], after["observed_outcomes"]),
        "success_p95_ms": (
            cohort["success_latency"]["p95_us"] / 1000
            if cohort["success_latency"]["p95_us"] is not None
            else None
        ),
        "failure_p95_ms": (
            cohort["failure_latency"]["p95_us"] / 1000
            if cohort["failure_latency"]["p95_us"] is not None
            else None
        ),
        "attempts_per_request": ratio(c["physical_attempts"], c["logical_arrivals"]),
        "consumed_work_units": sum(v2["stage_consumed_work_milli"]) / 1000,
        "work_units_per_request": sum(v2["stage_consumed_work_milli"])
        / 1000
        / c["logical_arrivals"],
        "unsuccessful_work_units": v2["unsuccessful_terminal_attempt_work_milli"]
        / 1000,
        "outstanding_work_units": v2["outstanding_attempt_work_milli"] / 1000,
        "stage_busy_worker_seconds": sum(v2["stage_busy_worker_us"]) / 1e6,
        "budget_denied_ratio": c.get("logical_budget_denied", 0)
        / c["logical_arrivals"],
    }


GROUP_COLUMNS = [
    "block",
    "rho",
    "failure_rate",
    "trigger_seconds",
    "timeout_us",
    "demand_identity",
    "client_contract",
    "reference_trigger_seconds",
]
MEASURES = [
    "success_ratio",
    "during_success_ratio",
    "post_success_ratio",
    "success_p95_ms",
    "failure_p95_ms",
    "attempts_per_request",
    "work_units_per_request",
    "budget_denied_ratio",
    "during_reference_success_ratio",
    "early_post_success_ratio",
    "matched_incident_excess_failure_ratio",
    "matched_early_post_excess_failure_ratio",
    "source_admissions_per_physical_attempt",
]


def metric_aggregates_sql() -> str:
    # Legacy summaries have all-null new fields, inferred as JSON by DuckDB.
    # Their metrics are unavailable, not zero and not a schema error.
    return ", ".join(
        f"avg(CAST({metric} AS DOUBLE)) AS {metric}" for metric in MEASURES
    )


def wilson_interval(successes: int, total: int) -> tuple[float | None, float | None]:
    """95% Wilson binomial interval over independent eligible seed outcomes."""
    if not 0 <= successes <= total:
        raise ValueError("invalid binomial counts")
    if total == 0:
        return None, None
    z = 1.959963984540054
    p = successes / total
    denominator = 1 + z * z / total
    center = (p + z * z / (2 * total)) / denominator
    half = (
        z * math.sqrt(p * (1 - p) / total + z * z / (4 * total * total)) / denominator
    )
    return max(0.0, center - half), min(1.0, center + half)


def attach_matched_controls(rows: list[dict]) -> None:
    def key(row):
        return tuple(row[k] for k in GROUP_COLUMNS if k != "trigger_seconds") + (
            row["seed"],
            row["arm"],
        )

    controls = {key(row): row for row in rows if row["trigger_seconds"] == 0}
    for row in rows:
        if row["reference_trigger_seconds"] <= 0:
            continue
        control = controls.get(key(row))
        if control is None:
            raise ValueError("calendar-aligned no-trigger control missing")
        for phase, metric in (
            ("incident", "during_reference_success_ratio"),
            ("early_post", "early_post_success_ratio"),
        ):
            if row[metric] is not None and control[metric] is not None:
                row[f"matched_{phase}_excess_failure_ratio"] = (
                    control[metric] - row[metric]
                )


def paired_analysis(rows: list[dict], protocol: dict) -> list[dict]:
    index = {(tuple(r[k] for k in GROUP_COLUMNS), r["seed"], r["arm"]): r for r in rows}
    result = []
    groups = sorted({tuple(r[k] for k in GROUP_COLUMNS) for r in rows})
    rng = random.Random(protocol["analysis"]["bootstrap_seed"])
    for group in groups:
        seeds = sorted(
            {r["seed"] for r in rows if tuple(r[k] for k in GROUP_COLUMNS) == group}
        )
        arms = sorted(
            {r["arm"] for r in rows if tuple(r[k] for k in GROUP_COLUMNS) == group}
        )
        for reference_arm in ("full_jitter", "no_retry"):
            if reference_arm not in arms:
                continue
            for arm in arms:
                if arm == reference_arm:
                    continue
                for metric in [
                    "success_ratio",
                    "during_success_ratio",
                    "post_success_ratio",
                    "work_units_per_request",
                    "success_p95_ms",
                    "recovered_600",
                    "during_reference_success_ratio",
                    "early_post_success_ratio",
                    "matched_incident_excess_failure_ratio",
                    "failure_p95_ms",
                    "budget_denied_ratio",
                ]:
                    differences = []
                    for seed in seeds:
                        a, b = (
                            index[(group, seed, arm)],
                            index[(group, seed, reference_arm)],
                        )
                        if metric == "recovered_600" and not (
                            a["baseline_eligible"] and b["baseline_eligible"]
                        ):
                            continue
                        if a.get(metric) is not None and b.get(metric) is not None:
                            differences.append(a[metric] - b[metric])
                    if not differences:
                        continue
                    boot = sorted(
                        statistics.mean(rng.choices(differences, k=len(differences)))
                        for _ in range(protocol["analysis"]["bootstrap_replicates"])
                    )
                    result.append(
                        {
                            **dict(zip(GROUP_COLUMNS, group)),
                            "arm": arm,
                            "reference_arm": reference_arm,
                            "metric": metric,
                            "pairs": len(differences),
                            "mean_difference": statistics.mean(differences),
                            "planned_pairs": len(seeds),
                            "ci_low": boot[int(len(boot) * 0.025)],
                            "ci_high": boot[int(len(boot) * 0.975)],
                            "interpretation": "exploratory_per_cell_no_multiplicity_adjustment",
                        }
                    )
    return result


def analyze(output: Path) -> None:
    protocol, _ = load_v2_protocol()
    plan, execution, runs = load_runs(output)
    if canonical_sha256(protocol) != plan["protocol_sha256"]:
        raise ValueError("current V2 protocol does not match this study")
    audit_controls(runs)
    rows = [run_row(r, protocol) for r in runs]
    if plan.get("followup_protocol"):
        if (
            canonical_sha256(plan["followup_protocol"])
            != plan["followup_protocol_sha256"]
        ):
            raise ValueError("frozen followup protocol checksum mismatch")
        attach_matched_controls(rows)
    rows.sort(key=lambda r: r["run_id"])
    write_json(output / "run_rows.json", rows)
    pairs = paired_analysis(rows, protocol)
    write_json(output / "paired_effects.json", pairs)
    extra_files = []
    datasets = ["runs", "cells", "paired_effects"]
    if plan.get("followup_protocol"):
        from retrycascade.followup_analysis import analyze_followup

        extra_files = analyze_followup(output, runs, rows, plan)
        datasets += ["recovery_features", "analytical_references", "incident_outcomes"]
    database_temp = output / "analysis.duckdb.tmp"
    # File names are bound as parameters, never interpolated from user SQL.
    with duckdb.connect(str(database_temp)) as db:
        db.execute("SET threads=1")
        db.execute(
            "CREATE OR REPLACE TABLE runs AS SELECT * FROM read_json_auto(?)",
            [str(output / "run_rows.json")],
        )
        db.execute(
            "CREATE OR REPLACE TABLE paired_effects AS SELECT * FROM read_json_auto(?)",
            [str(output / "paired_effects.json")],
        )
        for name in datasets[3:]:
            db.execute(
                f"CREATE OR REPLACE TABLE {name} AS SELECT * FROM read_json_auto(?)",
                [str(output / f"{name}.json")],
            )
        groups = ", ".join(GROUP_COLUMNS + ["arm"])
        measures = metric_aggregates_sql()
        db.execute(
            f"CREATE OR REPLACE TABLE cells AS SELECT {groups}, count(*) AS seeds, "
            "count(recovered_600) FILTER (WHERE baseline_eligible) AS recovery_observed, "
            "count(*) FILTER (WHERE recovered_600 AND baseline_eligible) AS recovery_count, "
            "count(recovered_300) FILTER (WHERE baseline_eligible) AS recovery_observed_300, "
            "count(*) FILTER (WHERE recovered_300 AND baseline_eligible) AS recovery_count_300, "
            "count(recovered_900) FILTER (WHERE baseline_eligible) AS recovery_observed_900, "
            "count(*) FILTER (WHERE recovered_900 AND baseline_eligible) AS recovery_count_900, "
            "count(*) FILTER (WHERE baseline_eligible) AS eligible_baselines, "
            f"{measures} FROM runs GROUP BY {groups}"
        )
        query = "SELECT * FROM cells ORDER BY block, rho, failure_rate, trigger_seconds, arm"
        names = [item[0] for item in db.execute(query).description]
        cells = [dict(zip(names, row)) for row in db.fetchall()]
        for cell in cells:
            # Do not rank survivors' latency or floating-point noise as a winner.
            cell["descriptive_mean_dominated_by"] = ""
            cell["ranking_status"] = (
                "not_ranked_use_paired_outcomes_and_practical_thresholds"
            )
            interval = wilson_interval(
                cell["recovery_count"], cell["recovery_observed"]
            )
            cell["recovery_probability_wilson_low"] = interval[0]
            cell["recovery_probability_wilson_high"] = interval[1]
        write_json(output / "cells.json", cells)
        db.execute(
            "CREATE OR REPLACE TABLE cells AS SELECT * FROM read_json_auto(?)",
            [str(output / "cells.json")],
        )
        for name in datasets:
            path = output / f"{name}.parquet"
            temp = path.with_suffix(".parquet.tmp")
            db.execute(
                f"COPY {name} TO ? (FORMAT PARQUET, COMPRESSION ZSTD)", [str(temp)]
            )
            temp.replace(path)
            count = db.execute(
                "SELECT count(*) FROM read_parquet(?)", [str(path)]
            ).fetchone()[0]
            expected = db.execute(f"SELECT count(*) FROM {name}").fetchone()[0]
            if count != expected:
                raise ValueError("Parquet round-trip row count differs")
    database_temp.replace(output / "analysis.duckdb")
    report(output, cells, plan)
    write_json(
        output / "analysis_manifest.json",
        {
            "status": "complete",
            "plan_sha256": digest_file(output / "plan.json"),
            "binary_sha256": execution["binary_sha256"],
            "analysis_source_sha256": LOADED_SOURCE_SHA256,
            "files": {
                name: digest_file(output / name)
                for name in [
                    "run_rows.json",
                    "cells.json",
                    "paired_effects.json",
                    "runs.parquet",
                    "cells.parquet",
                    "paired_effects.parquet",
                    "analysis.duckdb",
                    "report.html",
                    "report.md",
                    "success_by_failure.png",
                    "success_by_failure.svg",
                    "success_vs_work.png",
                    "success_vs_work.svg",
                ]
                + extra_files
                + [f"{name}.parquet" for name in datasets[3:]]
            },
            "analysis_module_sha256": {
                name: digest_file(Path(__file__).with_name(name))
                for name in ("recovery_features.py", "followup_analysis.py")
                if Path(__file__).with_name(name).exists()
            },
            "run_count": len(runs),
            "common_trace_control_audit": "pass",
        },
    )
    print(f"Analysis complete: {len(runs)} runs; {output / 'report.html'}", flush=True)


def report(output: Path, cells: list[dict], plan: dict) -> None:
    import matplotlib

    matplotlib.use("Agg")
    matplotlib.rcParams["svg.hashsalt"] = "retrycascade-v2"
    import matplotlib.pyplot as plt

    plot_cells = cells
    if plan.get("followup_protocol"):
        max_reference = max(c["reference_trigger_seconds"] for c in cells)
        plot_cells = [
            c
            for c in cells
            if c["client_contract"] == "strict_rpc"
            and (
                c["trigger_seconds"] > 0
                or c["reference_trigger_seconds"] == max_reference
            )
        ]
    arms = sorted({c["arm"] for c in plot_cells})
    rhos = sorted({c["rho"] for c in plot_cells if c["block"] == "main"})
    durations = sorted(
        {c["trigger_seconds"] for c in plot_cells if c["block"] == "main"}
    )
    fig, axes = plt.subplots(
        len(durations),
        len(rhos),
        figsize=(5 * len(rhos), 4 * len(durations)),
        squeeze=False,
    )
    for i, duration in enumerate(durations):
        for j, rho in enumerate(rhos):
            ax = axes[i][j]
            for arm in arms:
                group = sorted(
                    [
                        c
                        for c in plot_cells
                        if c["block"] == "main"
                        and c["rho"] == rho
                        and c["trigger_seconds"] == duration
                        and c["arm"] == arm
                    ],
                    key=lambda c: c["failure_rate"],
                )
                if group:
                    ax.plot(
                        [100 * c["failure_rate"] for c in group],
                        [100 * c["success_ratio"] for c in group],
                        marker="o",
                        label=arm,
                    )
            ax.set(
                title=f"rho={rho:.2f}, trigger={duration:g}s",
                xlabel="Transient failure (%)",
                ylabel="Mature request success (%)",
                ylim=(0, 101),
            )
            ax.grid(alpha=0.25)
    axes[0][0].legend(fontsize=8)
    fig.suptitle(
        f"RetryCascade V2 — synthetic discovery ({len(plan['seeds'])} seeds)\n"
        "Seed means; not production predictions",
        fontsize=11,
    )
    fig.tight_layout()
    fig.savefig(output / "success_by_failure.png", dpi=140, bbox_inches="tight")
    fig.savefig(
        output / "success_by_failure.svg", bbox_inches="tight", metadata={"Date": None}
    )
    plt.close(fig)
    fig, axes = plt.subplots(
        len(durations),
        len(rhos),
        figsize=(5 * len(rhos), 4 * len(durations)),
        squeeze=False,
    )
    for i, duration in enumerate(durations):
        for j, rho in enumerate(rhos):
            ax = axes[i][j]
            for arm in arms:
                group = [
                    c
                    for c in plot_cells
                    if c["block"] == "main"
                    and c["rho"] == rho
                    and c["trigger_seconds"] == duration
                    and c["arm"] == arm
                ]
                ax.scatter(
                    [c["work_units_per_request"] for c in group],
                    [100 * c["success_ratio"] for c in group],
                    label=arm,
                    alpha=0.8,
                )
            ax.set(
                title=f"rho={rho:.2f}, trigger={duration:g}s",
                xlabel="Consumed work units / request",
                ylabel="Mature request success (%)",
                ylim=(0, 101),
            )
            ax.grid(alpha=0.25)
    axes[0][0].legend(fontsize=8)
    fig.tight_layout()
    fig.savefig(output / "success_vs_work.png", dpi=140, bbox_inches="tight")
    fig.savefig(
        output / "success_vs_work.svg", bbox_inches="tight", metadata={"Date": None}
    )
    plt.close(fig)
    headers = [
        "client_contract",
        "rho",
        "failure_rate",
        "trigger_seconds",
        "reference_trigger_seconds",
        "arm",
        "seeds",
        "success_ratio",
        "during_success_ratio",
        "during_reference_success_ratio",
        "early_post_success_ratio",
        "matched_incident_excess_failure_ratio",
        "post_success_ratio",
        "success_p95_ms",
        "failure_p95_ms",
        "budget_denied_ratio",
        "attempts_per_request",
        "work_units_per_request",
        "recovery_count",
        "recovery_observed",
        "recovery_probability_wilson_low",
        "recovery_probability_wilson_high",
        "recovery_count_300",
        "recovery_count_900",
        "eligible_baselines",
    ]

    def fmt(value):
        return (
            "—"
            if value is None
            else f"{value:.4f}"
            if isinstance(value, float)
            else str(value)
        )

    table = (
        "<table><thead><tr>"
        + "".join(f"<th>{html.escape(k)}</th>" for k in headers)
        + "</tr></thead><tbody>"
    )
    for cell in cells:
        table += (
            "<tr>"
            + "".join(f"<td>{html.escape(fmt(cell[k]))}</td>" for k in headers)
            + "</tr>"
        )
    table += "</tbody></table>"
    notes = (
        f"{len(plan['jobs'])} koşu; {len(plan['seeds'])} discovery seed. "
        "Sentetik, keşifsel sonuçlar; V1 H1 testi ve production önerisi değildir. "
        "Başarı/latency için son deadline cohort'u bütünüyle dışarıda tutulur; "
        "kısmi server work horizon'da dahil edilir. İş miktarı CPU veya parasal maliyet değildir. "
        "Recovery policy'ye özel baseline'a göredir; düşük başlangıç başarısı ile kolaylaşabilir. "
        "Baseline eligibility mutlak %95 başarı eşiği kullanmaz. "
        "No-trigger satırlarında recovery yalnız referans ölçümüdür. "
        "İlk retry 0–25 ms, ikinci 0–50 ms jitter kullanır; 200 ms cap aktif değildir. "
        "Paired seed bootstrap aralıkları paired_effects.parquet içinde; çoklu kıyaslar keşifseldir. "
        "Az seed ile dar görünen aralıklar genel belirsizlik garantisi değildir. "
        "İki grafik seed ortalamalarını gösterir; farklı failure oranlarının scatter noktaları "
        "tek operating point gibi karşılaştırılmamalıdır."
    )
    notes += " Otomatik kazanan/dominance sıralaması kapalıdır. Başarılı istek gecikmesi yalnız başarılı kullanıcıların koşullu ölçümüdür; failure gecikmesi ve reddedilme birlikte okunmalıdır. Work ve attempt oranları tüm koşunun fiziksel yüküdür; mature cohort başarı paydasıyla aynı kullanıcı grubunu temsil etmez."
    if plan.get("followup_protocol"):
        notes += " Toparlanma indeksi tanımlı goodput ve kuyruk hedeflerinin betimleyici 0–100 özetidir; öğrenilmiş/predictive bir model değildir. 100, bütün kullanıcıların başarılı olduğu veya resmi sustained recovery gerçekleştiği anlamına gelmez. Zaman serisi 30s nominal çözünürlüktedir; incident ve ilk 60s sınırları ayrıca bölünür. EWMA yalnız geçmiş tamamlanmış binleri kullanır. Gölgelendirme seed min–max aralığıdır, güven aralığı değildir. Client contract varyantları alternatif arayüz sözleşmeleridir; eski strict-RPC modelinin düzeltmesi değildir."
    page = '<!doctype html><html lang="tr"><meta charset="utf-8"><title>RetryCascade V2</title>'
    page += "<style>body{font:15px/1.5 system-ui;margin:2rem;color:#182533}img{max-width:100%}table{border-collapse:collapse;font-size:12px}td,th{border:1px solid #ddd;padding:6px}th{position:sticky;top:0;background:#eee}.scroll{overflow:auto}a{color:#155db0}</style>"
    page += (
        "<h1>RetryCascade V2 — fayda ve bedel karşılaştırması</h1><p>"
        + html.escape(notes)
        + "</p>"
    )
    page += '<p><a href="runs.parquet">Koşular Parquet</a> · <a href="cells.parquet">Hücreler Parquet</a> · <a href="paired_effects.json">Eşlenmiş etkiler</a></p>'
    if plan.get("followup_protocol"):
        page += '<h2>Zaman içinde toparlanma: indeks ve ayrı bileşenler</h2><img src="recovery_curves.png"><h2>Arıza anındaki ve ilk dakikadaki kullanıcı başarısı</h2><img src="incident_outcomes.png"><p><a href="recovery_features.parquet">Zaman serisi özellikleri</a> · <a href="analytical_references.json">Analitik kontroller</a></p>'
    page += '<h2>Geçici hata oranına göre kullanıcı başarısı</h2><img src="success_by_failure.png">'
    page += (
        '<h2>Başarı ve işlem yükü</h2><img src="success_vs_work.png"><h2>Tüm hücreler</h2><div class="scroll">'
        + table
        + "</div></html>"
    )
    (output / "report.html").write_text(page, encoding="utf-8")
    lines = [
        "# RetryCascade V2 keşif raporu",
        "",
        notes,
        "",
        "![Başarı eğrileri](success_by_failure.png)",
        "",
        "![Başarı ve iş](success_vs_work.png)",
        "",
        "|" + "|".join(headers) + "|",
        "|" + "|".join("---" for _ in headers) + "|",
    ]
    lines += ["|" + "|".join(fmt(c[k]) for k in headers) + "|" for c in cells]
    if plan.get("followup_protocol"):
        lines[4:4] = [
            "![Toparlanma zaman serisi](recovery_curves.png)",
            "",
            "![Arıza dönemleri](incident_outcomes.png)",
            "",
        ]
    (output / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def run_study(
    output: Path,
    preset: str,
    workers: int,
    retry_failed: bool,
    limit: int | None = None,
) -> int:
    with study_lock(output):
        return _run_study(output, preset, workers, retry_failed, limit)


def _run_study(
    output: Path, preset: str, workers: int, retry_failed: bool, limit: int | None
) -> int:
    plan = make_plan(preset)
    output.mkdir(parents=True, exist_ok=True)
    freeze_plan(output, plan)
    binary = ROOT / "target/release/retrycascade-cli"
    fingerprint = digest_file(binary)
    execution_path = output / "execution.json"
    if execution_path.exists():
        previous = json.loads(execution_path.read_text())
        if previous["binary_sha256"] != fingerprint:
            raise ValueError("binary changed: use a new output directory")
    else:
        frozen_binary = output / "simulator" / binary.name
        frozen_binary.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(binary, frozen_binary)
        if digest_file(frozen_binary) != fingerprint:
            raise ValueError("binary changed during snapshot")
        source_hashes = {}
        for pattern in (
            "crates/**/*.rs",
            "crates/**/Cargo.toml",
            "python/retrycascade/*.py",
            "configs/*.yaml",
            "schemas/*.json",
        ):
            for source in sorted(ROOT.glob(pattern)):
                relative = source.relative_to(ROOT)
                destination = output / "source_snapshot" / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, destination)
                source_hashes[str(relative)] = digest_file(destination)
        write_json(
            execution_path,
            {
                "binary_sha256": fingerprint,
                "git_commit": subprocess.check_output(
                    ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
                ).strip(),
                "git_status": subprocess.check_output(
                    ["git", "status", "--porcelain"], cwd=ROOT, text=True
                ),
                "cargo_lock_sha256": digest_file(ROOT / "Cargo.lock"),
                "python_lock_sha256": digest_file(ROOT / "python/uv.lock"),
                "runner_sha256": LOADED_SOURCE_SHA256,
                "frozen_binary": str(frozen_binary.relative_to(output)),
                "source_snapshot_sha256": source_hashes,
                "python_version": sys.version,
                "duckdb_version": duckdb.__version__,
                "rustc_version": subprocess.check_output(
                    ["rustc", "--version"], text=True
                ).strip(),
                "confirmation_seeds_used": False,
            },
        )
    execution = json.loads(execution_path.read_text())
    if execution.get("frozen_binary"):
        binary = output / execution["frozen_binary"]
        if digest_file(binary) != fingerprint:
            raise ValueError("frozen simulator checksum mismatch")
    jobs = plan["jobs"][:limit] if limit else plan["jobs"]
    completed = 0
    failures = []
    start = time.monotonic()
    print(
        f"{preset}: {len(jobs)}/{len(plan['jobs'])} runs, {workers} workers", flush=True
    )
    with ThreadPoolExecutor(max_workers=workers) as pool:
        pending = {
            pool.submit(
                execute_job, job, binary, output, fingerprint, retry_failed
            ): job
            for job in jobs
        }
        for future in as_completed(pending):
            try:
                future.result()
                completed += 1
            except Exception as error:  # noqa: BLE001 — retain every failed job while others finish
                failures.append(
                    {
                        "run_id": pending[future]["config"]["run_config_sha256"],
                        "error": str(error),
                    }
                )
            total = completed + len(failures)
            if total % 25 == 0 or total == len(jobs):
                print(
                    f"{completed} complete, {len(failures)} failed; {time.monotonic() - start:.1f}s",
                    flush=True,
                )
    write_json(
        output / "execution_status.json",
        {
            "planned": len(plan["jobs"]),
            "completed_in_invocation": completed,
            "failures": failures,
            "partial_invocation": bool(limit),
            "elapsed_seconds": time.monotonic() - start,
        },
    )
    if failures:
        print(
            "Failures retained; healthy artifacts preserved. See execution_status.json",
            file=sys.stderr,
        )
        return 1
    if not limit:
        analyze(output)
    return 0


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("plan", "run", "analyze", "query", "verify", "reproduce"):
        p = sub.add_parser(name)
        p.add_argument("--output", type=Path, required=True)
        if name in ("plan", "run"):
            p.add_argument(
                "--preset",
                choices=["smoke", "pilot", "discovery", "followup", "followup-smoke"],
                default="pilot",
            )
        if name == "run":
            p.add_argument("--workers", type=int, default=4)
            p.add_argument("--retry-failed", action="store_true")
            p.add_argument(
                "--limit", type=int, help="partial run for resume checks; no analysis"
            )
        if name == "query":
            p.add_argument("--sql", required=True)
        if name == "reproduce":
            p.add_argument("--run-id", help="default: first frozen-plan run")
    args = parser.parse_args()
    output = args.output.resolve()
    if args.command == "plan":
        plan = make_plan(args.preset)
        with study_lock(output):
            freeze_plan(output, plan)
        print(f"Frozen plan: {len(plan['jobs'])} runs, {output / 'plan.json'}")
    elif args.command == "run":
        if args.workers < 1 or args.limit is not None and args.limit < 1:
            parser.error("workers and limit must be positive")
        raise SystemExit(
            run_study(output, args.preset, args.workers, args.retry_failed, args.limit)
        )
    elif args.command == "analyze":
        with study_lock(output):
            analyze(output)
    elif args.command == "verify":
        verify_analysis(output)
        _, _, runs = load_runs(output)
        audit_controls(runs)
        print(f"Verified {len(runs)} raw runs, derived artifacts and common traces")
    elif args.command == "reproduce":
        plan, execution, _ = load_runs(output)
        candidates = [
            job
            for job in plan["jobs"]
            if args.run_id is None or job["config"]["run_config_sha256"] == args.run_id
        ]
        if not candidates:
            raise ValueError("run-id is not in the frozen plan")
        job = candidates[0]
        run_id = job["config"]["run_config_sha256"]
        directory = output / "runs" / run_id
        binary = (
            output / execution["frozen_binary"]
            if execution.get("frozen_binary")
            else ROOT / "target/release/retrycascade-cli"
        )
        if digest_file(binary) != execution["binary_sha256"]:
            raise ValueError(
                "the original simulator binary is required to reproduce this run"
            )
        original = verify_artifact(directory, job["config"], execution["binary_sha256"])
        fresh = json.loads(
            subprocess.check_output(
                [str(binary), str(directory / "resolved_run_config.json")], text=True
            )
        )
        validate_summary(fresh, job["config"])
        if original != fresh:
            raise ValueError(
                "deterministic reproduction differs from the original summary"
            )
        print(f"Reproduced {run_id}: identical summary {fresh['summary_sha256']}")
    else:
        with study_lock(output, shared=True):
            verify_analysis(output)
            with duckdb.connect(str(output / "analysis.duckdb"), read_only=True) as db:
                cursor = db.execute(args.sql)
                columns = [item[0] for item in cursor.description]
                print(
                    json.dumps(
                        [dict(zip(columns, row)) for row in cursor.fetchall()],
                        indent=2,
                        default=str,
                    )
                )


if __name__ == "__main__":
    main()
