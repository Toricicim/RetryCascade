"""Reproducible exploratory pilot. Pilot seeds never enter primary analysis.

Run from python/:
    .venv/bin/python -m retrycascade.pilot --output ../output/pilot
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from copy import deepcopy
import hashlib
import html
import json
import math
from pathlib import Path
import random
import statistics
import subprocess
import time

from retrycascade.protocol import canonical_sha256, load_and_validate
from retrycascade.resolve_config import resolve_run_config, validate_resolved_run_config

ROOT = Path(__file__).resolve().parents[2]
POLICIES = ["immediate", "exponential", "full_jitter", "retry_budget"]


def digest_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
    temporary.replace(path)


def validate_summary(summary: dict, config: dict) -> None:
    if summary["provenance"]["run_config_sha256"] != config["run_config_sha256"]:
        raise ValueError("summary/config provenance mismatch")
    if not summary["lifecycle_ledger_closed"]:
        raise ValueError("lifecycle ledger did not close")
    counts = summary["counts"]
    logical = sum(counts[key] for key in (
        "unique_goodput", "logical_deadline_abandoned", "logical_attempts_exhausted",
        "logical_outstanding_at_horizon",
    )) + counts.get("logical_budget_denied", 0)
    physical = sum(counts[key] for key in (
        "unique_goodput", "late_completions", "duplicate_completions", "queue_rejections",
        "attempt_outstanding_at_horizon",
    )) + counts.get("pre_admission_failures", 0)
    if logical != counts["logical_arrivals"] or physical != counts["physical_attempts"]:
        raise ValueError("terminal counters do not conserve requests/attempts")
    for cohort in summary["diagnostics"]["arrival_cohorts"]:
        terminal = sum(cohort[key] for key in (
            "unique_on_time_successes", "deadline_abandoned", "attempts_exhausted",
            "outstanding_at_horizon",
        )) + cohort.get("budget_denied", 0)
        if terminal != cohort["logical_arrivals"]:
            raise ValueError("arrival-cohort terminal counters do not conserve requests")
    if config.get("extensions") is not None:
        metrics = summary["v2_metrics"]
        total_work = sum(metrics["stage_consumed_work_milli"])
        classified = sum(metrics[key] for key in (
            "useful_attempt_work_milli", "unsuccessful_terminal_attempt_work_milli",
            "outstanding_attempt_work_milli",
        ))
        if total_work != classified:
            raise ValueError("consumed work conservation mismatch")
        for cohort in metrics["outcome_cohorts"]:
            if (cohort["successes"] + cohort["failures"] != cohort["observed_outcomes"]
                or cohort["observed_outcomes"] + cohort["right_censored"] != cohort["arrivals"]):
                raise ValueError("mature cohort conservation mismatch")
        for i, busy in enumerate(metrics["stage_busy_worker_us"]):
            if not 0 <= busy <= config["stages"][i]["workers"] * config["run_end_us"]:
                raise ValueError("worker busy time exceeds capacity")
    unhashed = deepcopy(summary)
    expected = unhashed["summary_sha256"]
    unhashed["summary_sha256"] = ""
    actual = hashlib.sha256(json.dumps(
        unhashed, ensure_ascii=False, separators=(",", ":"),
    ).encode()).hexdigest()
    if actual != expected:
        raise ValueError("summary semantic checksum mismatch")


def make_jobs(protocol: dict, schema: dict) -> list[dict]:
    """240 trigger + 60 no-trigger + 27 no-retry controls = 327 runs."""
    seeds = protocol["experiment"]["pilot_seeds"]
    if set(seeds) & set(protocol["experiment"]["main_seeds"]):
        raise ValueError("pilot and main seed banks overlap")
    jobs = []
    for rho in protocol["workload"]["arrival_process"]["rho_basis_points"]:
        for duration in [0, *protocol["trigger"]["durations_us"]]:
            for policy in POLICIES:
                for seed in seeds:
                    selection = {
                        "schema_version": protocol["schema_version"],
                        "analysis_eligible": True,
                        "seed": seed,
                        "rho_basis_points": rho,
                        "policy": policy,
                        "trigger_enabled": duration != 0,
                        "trigger_duration_us": duration,
                    }
                    config = resolve_run_config(protocol, selection)
                    # This metadata is outside the strict simulator config.
                    metadata = {
                        "scenario": "standard", "rho": rho, "duration_us": duration,
                        "policy": policy, "seed": seed, "phase": "exploratory_pilot",
                    }
                    validate_resolved_run_config(config, schema)
                    jobs.append({"metadata": metadata, "config": config})

    for rho in protocol["analysis"]["h1"]["high_stress_rho_basis_points"]:
        for duration in [0, *protocol["analysis"]["h1"]["high_stress_trigger_durations_us"]]:
            for seed in seeds:
                selection = {
                    "schema_version": protocol["schema_version"], "analysis_eligible": True,
                    "seed": seed, "rho_basis_points": rho, "policy": "immediate",
                    "trigger_enabled": duration != 0, "trigger_duration_us": duration,
                }
                config = resolve_run_config(protocol, selection)
                config["retry_contract"]["max_attempts_including_initial"] = 1
                config["run_config_sha256"] = canonical_sha256(config)
                validate_resolved_run_config(config, schema)
                jobs.append({"metadata": {
                    "scenario": "no_retry", "rho": rho, "duration_us": duration,
                    "policy": "no_retry", "seed": seed, "phase": "exploratory_pilot",
                }, "config": config})
    return jobs


def execute(job: dict, binary: Path, output: Path) -> dict:
    config = job["config"]
    directory = output / "runs" / config["run_config_sha256"]
    directory.mkdir(parents=True, exist_ok=True)
    config_path = directory / "resolved_run_config.json"
    summary_path = directory / "run_summary.json"
    write_json(config_path, config)
    write_json(directory / "selection.json", job["metadata"])
    start = time.monotonic()
    if summary_path.exists():
        summary = json.loads(summary_path.read_text())
        validate_summary(summary, config)
    else:
        process = subprocess.run([str(binary), str(config_path)], capture_output=True,
                                 text=True, timeout=180)
        if process.returncode:
            write_json(directory / "failure.json", {"stderr": process.stderr,
                       "returncode": process.returncode})
            raise RuntimeError(f"run failed: {job['metadata']}: {process.stderr}")
        summary = json.loads(process.stdout)
        validate_summary(summary, config)
        write_json(summary_path, summary)
    return {"metadata": job["metadata"], "config": config, "summary": summary,
            "elapsed_seconds": time.monotonic() - start}


def ratio(numerator: int, denominator: int) -> float | None:
    return numerator / denominator if denominator else None


def baseline_diagnostic(run: dict, protocol: dict) -> dict:
    windows = run["summary"]["diagnostics"]["baseline_windows"]
    goodputs = [w["unique_goodput"] / ((w["end_us"] - w["start_us"]) / 1e6)
                for w in windows]
    queues = [w["queue_area_slot_us"] / (w["end_us"] - w["start_us"])
              for w in windows]
    xs = [((w["start_us"] + w["end_us"]) / 2) / 1e6 for w in windows]
    mx, my = statistics.mean(xs), statistics.mean(queues)
    denominator = sum((x - mx) ** 2 for x in xs)
    slope = sum((x - mx) * (y - my) for x, y in zip(xs, queues)) / denominator
    mean_goodput = statistics.mean(goodputs)
    cv = statistics.pstdev(goodputs) / mean_goodput if mean_goodput else math.inf
    cohort = run["summary"]["diagnostics"]["baseline_cohort"]
    # Conservative pre-trigger-only ratio: late baseline arrivals may still be
    # open at trigger start. Do not let post-trigger outcomes affect eligibility.
    success = ratio(cohort["successes_by_interval_end"], cohort["logical_arrivals"])
    rules = protocol["baseline"]["stability_check"]
    checks = {
        "success_ratio_pass": success is not None
        and success >= rules["minimum_success_ratio_basis_points"] / 1e4,
        "goodput_cv_pass": cv <= rules["maximum_goodput_cv_basis_points"] / 1e4,
        "queue_slope_pass": abs(slope) <=
        rules["maximum_absolute_queue_slope_milli_slots_per_second"] / 1e3,
    }
    return {"stable": all(checks.values()), "success_ratio": success,
            "goodput_cv": cv if math.isfinite(cv) else None,
            "queue_slope_slots_per_second": slope, **checks}


def wilson(successes: int, trials: int) -> list[float] | None:
    if not trials:
        return None
    z = 1.959963984540054
    p = successes / trials
    denominator = 1 + z * z / trials
    center = (p + z * z / (2 * trials)) / denominator
    radius = z * math.sqrt(p * (1 - p) / trials + z * z / (4 * trials**2)) / denominator
    return [max(0, center - radius), min(1, center + radius)]


def recovered(run: dict, horizon: int = 600_000_000) -> bool | None:
    return next(item["recovered_by_horizon"] for item in
                run["summary"]["recovery"]["sensitivity_horizons"]
                if item["horizon_us"] == horizon)


def cohort_success(run: dict, name: str) -> float | None:
    cohort = next(c for c in run["summary"]["diagnostics"]["arrival_cohorts"]
                  if c["name"] == name)
    return ratio(cohort["unique_on_time_successes"], cohort["logical_arrivals"])


def recovery_sensitivity(run: dict, goodput_bp: int, queue_bp: int,
                         queue_only: bool = False) -> bool | None:
    summary = run["summary"]
    if not summary["recovery"]["baseline_reference_available"]:
        return None
    g0 = summary["recovery"]["baseline_g0_milli_per_second"]
    q0 = summary["recovery"]["baseline_q0_milli_slots"]
    capacity = sum(stage["queue_capacity_waiting"] for stage in run["config"]["stages"])
    streak = 0
    for window in summary["recovery"]["windows"]:
        if window["end_us"] > summary["trigger_end_us"] + 600_000_000:
            break
        # Sensitivity uses emitted milli-rounded metrics; primary uses exact
        # Rust arithmetic. Never substitute this approximation for primary.
        queue_pass = window["mean_waiting_milli_slots"] <= q0 + capacity * queue_bp / 10
        goodput_pass = window["goodput_milli_per_second"] >= g0 * goodput_bp / 1e4
        streak = streak + 1 if queue_pass and (queue_only or goodput_pass) else 0
        if streak >= run["config"]["recovery"]["consecutive_non_overlapping_windows"]:
            return True
    return False


def analyze(runs: list[dict], protocol: dict) -> dict:
    indexed = {(r["metadata"]["scenario"], r["metadata"]["rho"],
                r["metadata"]["duration_us"], r["metadata"]["policy"],
                r["metadata"]["seed"]): r for r in runs}
    for run in runs:
        run["baseline"] = baseline_diagnostic(run, protocol)
        meta = run["metadata"]
        if meta["duration_us"]:
            control = indexed[(meta["scenario"], meta["rho"], 0, meta["policy"], meta["seed"])]
            # Window completions/arrival counts/queue areas all precede trigger;
            # cohort final outcomes are not included in this equality check.
            if run["summary"]["diagnostics"]["baseline_windows"] != \
                    control["summary"]["diagnostics"]["baseline_windows"]:
                raise ValueError(f"pre-trigger control mismatch: {meta}")
        if meta["policy"] == "retry_budget":
            jitter = indexed[(meta["scenario"], meta["rho"], meta["duration_us"],
                              "full_jitter", meta["seed"])]
            for key in ("arrival_trace_sha256", "potential_service_trace_sha256"):
                if jitter["summary"][key] != run["summary"][key]:
                    raise ValueError(f"common-random-number mismatch: {meta}")

    groups = {}
    for run in runs:
        meta = run["metadata"]
        groups.setdefault((meta["rho"], meta["duration_us"], meta["policy"]), []).append(run)
    cells = []
    for (rho, duration, policy), group in sorted(groups.items()):
        successes = sum(recovered(r) is True for r in group)
        observed = sum(recovered(r) is not None for r in group)
        cells.append({
            "rho": rho / 1e4, "duration_seconds": duration / 1e6, "policy": policy,
            "runs": len(group), "recovered_600": successes,
            "recovery_observed_runs": observed,
            "recovery_unavailable_runs": len(group) - observed,
            "recovery_wilson95": wilson(successes, observed),
            "stable_baselines": sum(r["baseline"]["stable"] for r in group),
            "mean_success_ratio": statistics.mean(ratio(r["summary"]["counts"]["unique_goodput"],
                                      r["summary"]["counts"]["logical_arrivals"]) for r in group),
            "mean_attempts_per_request": statistics.mean(ratio(
                r["summary"]["counts"]["physical_attempts"],
                r["summary"]["counts"]["logical_arrivals"]) for r in group),
            "mean_recovery_seconds_among_recovered": (statistics.mean(
                (r["summary"]["recovery"]["recovery_at_us"] -
                 r["summary"]["trigger_end_us"]) / 1e6 for r in group if recovered(r) is True)
                if successes else None),
            "mean_during_trigger_success": (statistics.mean(
                cohort_success(r, "during_trigger") for r in group) if duration else None),
            "mean_post_trigger_success": statistics.mean(cohort_success(r, "post_trigger")
                                                         for r in group),
            "mean_deadline_abandonment_ratio": statistics.mean(ratio(
                r["summary"]["counts"]["logical_deadline_abandoned"],
                r["summary"]["counts"]["logical_arrivals"]) for r in group),
            "mean_attempts_exhausted_ratio": statistics.mean(ratio(
                r["summary"]["counts"]["logical_attempts_exhausted"],
                r["summary"]["counts"]["logical_arrivals"]) for r in group),
            "mean_late_fraction_of_sink_completions": statistics.mean(ratio(
                r["summary"]["counts"]["late_completions"],
                r["summary"]["counts"]["unique_goodput"]
                + r["summary"]["counts"]["late_completions"]
                + r["summary"]["counts"]["duplicate_completions"]) for r in group),
        })
    paired = []
    seed_effects = {seed: [] for seed in protocol["experiment"]["pilot_seeds"]}
    for rho in protocol["analysis"]["h1"]["high_stress_rho_basis_points"]:
        for duration in protocol["analysis"]["h1"]["high_stress_trigger_durations_us"]:
            differences = []
            for seed in seed_effects:
                jitter = indexed[("standard", rho, duration, "full_jitter", seed)]
                budget = indexed[("standard", rho, duration, "retry_budget", seed)]
                a_recovery, b_recovery = recovered(jitter), recovered(budget)
                effect = (int(b_recovery) - int(a_recovery)
                          if a_recovery is not None and b_recovery is not None else None)
                seed_effects[seed].append(effect)
                differences.append({"seed": seed, "recovery_difference": effect,
                    "success_ratio_difference": ratio(budget["summary"]["counts"]["unique_goodput"],
                        budget["summary"]["counts"]["logical_arrivals"]) -
                        ratio(jitter["summary"]["counts"]["unique_goodput"],
                              jitter["summary"]["counts"]["logical_arrivals"]),
                    "during_trigger_success_difference": cohort_success(budget, "during_trigger")
                        - cohort_success(jitter, "during_trigger"),
                    "post_trigger_success_difference": cohort_success(budget, "post_trigger")
                        - cohort_success(jitter, "post_trigger"),
                    "both_baselines_stable": budget["baseline"]["stable"]
                        and jitter["baseline"]["stable"],
                })
            paired.append({"rho": rho / 1e4, "duration_seconds": duration / 1e6,
                           "pairs": differences})
    # Same seed reused across cells: average within seed before resampling.
    complete = all(effect is not None for effects in seed_effects.values() for effect in effects)
    blocks = [statistics.mean(effects) for effects in seed_effects.values()] if complete else []
    rng = random.Random(protocol["analysis"]["bootstrap"]["rng_seed"])
    boot = (sorted(statistics.mean(rng.choices(blocks, k=len(blocks)))
                   for _ in range(protocol["analysis"]["bootstrap"]["replicates"]))
            if complete else [])
    sensitivity = []
    for g in protocol["recovery"]["sensitivity"]["goodput_fractions_basis_points"]:
        for q in protocol["recovery"]["sensitivity"]["queue_margin_fractions_basis_points"]:
            effects = []
            for cell in paired:
                rho = round(cell["rho"] * 1e4)
                duration = round(cell["duration_seconds"] * 1e6)
                for pair in cell["pairs"]:
                    seed = pair["seed"]
                    a = indexed[("standard", rho, duration, "full_jitter", seed)]
                    b = indexed[("standard", rho, duration, "retry_budget", seed)]
                    a_result = recovery_sensitivity(a, g, q)
                    b_result = recovery_sensitivity(b, g, q)
                    effects.append(int(b_result) - int(a_result)
                                   if a_result is not None and b_result is not None else None)
            sensitivity.append({"goodput_bp": g, "queue_margin_bp": q,
                                "unavailable_pairs": sum(e is None for e in effects),
                                "mean_recovery_difference": statistics.mean(effects)
                                if all(e is not None for e in effects) else None})
    eligible = [pair for cell in paired for pair in cell["pairs"]
                if pair["both_baselines_stable"] and pair["recovery_difference"] is not None]
    contrasts = []
    for key, no_retry in indexed.items():
        scenario, rho, duration, _, seed = key
        if scenario != "no_retry":
            continue
        budget = indexed[("standard", rho, duration, "retry_budget", seed)]
        contrasts.append({"rho": rho / 1e4, "duration_seconds": duration / 1e6,
            "seed": seed,
            "success_difference_budget_minus_no_retry": ratio(
                budget["summary"]["counts"]["unique_goodput"],
                budget["summary"]["counts"]["logical_arrivals"]) - ratio(
                no_retry["summary"]["counts"]["unique_goodput"],
                no_retry["summary"]["counts"]["logical_arrivals"]),
            "post_trigger_success_difference": cohort_success(budget, "post_trigger")
                - cohort_success(no_retry, "post_trigger"),
        })
    return {"phase": "exploratory_pilot", "run_count": len(runs), "cells": cells,
            "high_stress_paired": paired,
            "stable_baseline_pair_count": len(eligible),
            "stable_subset_descriptive_recovery_difference": statistics.mean(
                p["recovery_difference"] for p in eligible) if eligible else None,
            "budget_vs_no_retry": contrasts,
            "high_stress_all_pairs_observed": complete,
            "high_stress_mean_recovery_difference": statistics.mean(blocks) if complete else None,
            "seed_block_effects": blocks,
            "exploratory_bootstrap95": ([boot[int(len(boot)*.025)], boot[int(len(boot)*.975)]]
                                         if complete else None),
            "recovery_definition_sensitivity_milli_rounded": sensitivity,
            "baseline_diagnostics": [{**r["metadata"], **r["baseline"]} for r in runs]}


def write_report(output: Path, analysis: dict, manifest: dict) -> None:
    lines = ["# RetryCascade keşif pilotu", "",
        f"Tamamlanan koşu: {analysis['run_count']}. Seed'ler: {manifest['pilot_seeds']}.", "",
        "Bu sonuçlar keşifseldir; pilot seed'leri ana hipotez testinde kullanılmaz.",
        "Üç seed ile yayın kararı veya anlamlılık iddiası verilmez. Her hücrede Wilson aralığı",
        "geniştir. Seed-blok bootstrap da yalnız üç bağımsız blokla sınırlıdır.", "",
        "## Hücre sonuçları", "",
        "| rho | Trigger (s) | Policy | Recovery / ölçülebilen | Referans yok | Stable baseline / 3 | Toplam başarı (%) | Trigger cohort başarı (%) | Post-trigger başarı (%) | Attempt/request |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for c in analysis["cells"]:
        during = "—" if c["mean_during_trigger_success"] is None else \
                 f"{100*c['mean_during_trigger_success']:.2f}"
        lines.append(f"| {c['rho']:.2f} | {c['duration_seconds']:g} | {c['policy']} | "
            f"{c['recovered_600']}/{c['recovery_observed_runs']} | "
            f"{c['recovery_unavailable_runs']} | {c['stable_baselines']}/{c['runs']} | "
            f"{100*c['mean_success_ratio']:.2f} | {during} | "
            f"{100*c['mean_post_trigger_success']:.2f} | {c['mean_attempts_per_request']:.3f} |")
    mean_effect = analysis['high_stress_mean_recovery_difference']
    effect_text = (f"{100*mean_effect:.2f} yüzde puan" if mean_effect is not None
                   else "hesaplanamaz (en az bir kolda baseline referansı yok)")
    lines += ["", "## Ana karşılaştırma: budget − full jitter", "",
        f"Yüksek-stres hücrelerinin eşit ağırlıklı recovery farkı: {effect_text}.",
        f"Seed-blok etkileri: {analysis['seed_block_effects']}.",
        f"Keşif bootstrap aralığı: {analysis['exploratory_bootstrap95']}.", "",
        f"Her iki kolda stable baseline bulunan eşlenmiş yüksek-stres koşuları: "
        f"{analysis['stable_baseline_pair_count']}/18. Bu alt kümenin betimleyici recovery farkı: "
        f"{analysis['stable_subset_descriptive_recovery_difference']}. Bu, tam H1 estimand'ının yerine geçmez.", "",
        "## Yorumlama sınırları", "",
        "- Trigger olmayan koşular ve max_attempts=1 kontrolleri ayrı raporlanır.",
        "- Baseline stability başarısız koşular silinmez; trigger-induced metastability iddiasında kullanılmaz.",
        "- G0/Q0 pencereleri her trigger koşusunda matched no-trigger koşusuyla bire bir kontrol edilir.",
        "- Ana recovery Rust'ın exact hesabıdır; threshold sensitivity milli-rounded çıktı üzerinden yaklaşık hesaplanır.",
        "- Cohort başarıları arrival zamanına göre gruplanır; post-trigger sonunda hâlâ açık istekler ayrıca sayılır.",
        "- Deadline abandonment ve attempts exhausted farklı son durumlardır; tek başına biri toplam kullanıcı kaybını göstermez.",
        "- Kuyruk reddi fiziksel attempt sayısıdır; tüketilen CPU/iş miktarına eşit değildir.",
        "- Model varsayımları gerçek sistemden kalibre edilmemiştir.",
        "- İstatistiksel sinyal, literatüre özgün katkı olduğunu tek başına göstermez.", "",
        "Ham koşular runs/ altında; provenance manifest.json, sayısal analiz analysis.json içindedir."]
    text = "\n".join(lines) + "\n"
    (output / "report.md").write_text(text, encoding="utf-8")
    # A portable browser view without extra rendering dependencies.
    (output / "report.html").write_text(
        '<!doctype html><meta charset="utf-8"><title>RetryCascade pilot</title>'
        '<style>body{font:15px/1.5 system-ui;margin:2rem}pre{white-space:pre-wrap}</style>'
        '<pre>' + html.escape(text) + '</pre>', encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "output" / "pilot")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--limit", type=int, help="debug subset, never a complete pilot")
    parser.add_argument("--analyze-only", action="store_true",
                        help="recompute analysis from existing verified artifacts; no new simulations")
    args = parser.parse_args()
    if args.workers < 1:
        parser.error("--workers must be positive")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    binary = ROOT / "target" / "release" / "retrycascade-cli"
    protocol = load_and_validate(ROOT / "configs" / "research_protocol.yaml",
                                ROOT / "schemas" / "research-protocol.schema.json", kind="protocol")
    schema = json.loads((ROOT / "schemas" / "resolved-run-config.schema.json").read_text())
    jobs = make_jobs(protocol, schema)
    manifest = {
        "phase": "exploratory_pilot", "protocol_sha256": canonical_sha256(protocol),
        "git_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "working_tree_status": subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True),
        "binary_sha256": digest_file(binary), "cargo_lock_sha256": digest_file(ROOT / "Cargo.lock"),
        "runner_sha256": digest_file(Path(__file__)),
        "pilot_seeds": protocol["experiment"]["pilot_seeds"], "planned_runs": len(jobs),
        "primary_seed_bank_used": False,
    }
    manifest_path = output / "manifest.json"
    if args.analyze_only:
        previous = json.loads(manifest_path.read_text())
        for key in ("binary_sha256", "protocol_sha256"):
            if previous[key] != manifest[key]:
                raise ValueError(f"artifact {key} does not match current pilot")
        runs = []
        for job in jobs:
            path = output / "runs" / job["config"]["run_config_sha256"] / "run_summary.json"
            summary = json.loads(path.read_text())
            validate_summary(summary, job["config"])
            runs.append({**job, "summary": summary})
        analysis = analyze(runs, protocol)
        initial_analysis = output / "analysis.initial.json"
        if not initial_analysis.exists():
            write_json(initial_analysis, json.loads((output / "analysis.json").read_text()))
        write_json(output / "analysis.json", analysis)
        write_report(output, analysis, previous)
        write_json(output / "analysis-manifest.json", {
            "execution_runner_sha256": previous["runner_sha256"],
            "analysis_runner_sha256": manifest["runner_sha256"],
            "analysis_sha256": digest_file(output / "analysis.json"),
            "reanalysis_of_completed_runs": len(runs),
        })
        previous["initial_analysis_artifact"] = "analysis.initial.json"
        previous["current_analysis_manifest"] = "analysis-manifest.json"
        write_json(manifest_path, previous)
        print(f"Reanalyzed {len(runs)} verified artifacts", flush=True)
        return
    if manifest_path.exists():
        previous = json.loads(manifest_path.read_text())
        for key in ("binary_sha256", "protocol_sha256", "runner_sha256"):
            if previous[key] != manifest[key]:
                raise ValueError(f"existing pilot {key} mismatch; use a new output directory")
    write_json(manifest_path, manifest)
    if args.limit:
        jobs = jobs[:args.limit]
    print(f"Pilot: {len(jobs)} runs, {args.workers} workers; output={output}", flush=True)
    start = time.monotonic()
    runs = []
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(execute, job, binary, output) for job in jobs]
        for future in as_completed(futures):
            runs.append(future.result())
            if len(runs) % 10 == 0 or len(runs) == len(jobs):
                print(f"Completed {len(runs)}/{len(jobs)} in {time.monotonic()-start:.1f}s", flush=True)
    runs.sort(key=lambda r: tuple(str(v) for v in r["metadata"].values()))
    if args.limit:
        print("Debug subset complete; no scientific analysis generated", flush=True)
        return
    analysis = analyze(runs, protocol)
    write_json(output / "analysis.json", analysis)
    write_report(output, analysis, manifest)
    # One rerun provides direct evidence that the executed binary is repeatable.
    first = runs[0]
    fresh = subprocess.check_output([str(binary), str(output / "runs" /
        first["config"]["run_config_sha256"] / "resolved_run_config.json")], text=True)
    if json.loads(fresh) != first["summary"]:
        raise ValueError("deterministic rerun mismatch")
    manifest.update({"completed_runs": len(runs), "elapsed_seconds": time.monotonic()-start,
                     "deterministic_rerun_pass": True,
                     "analysis_sha256": digest_file(output / "analysis.json")})
    write_json(manifest_path, manifest)
    print("Pilot complete: report.md, report.html, analysis.json", flush=True)


if __name__ == "__main__":
    main()
