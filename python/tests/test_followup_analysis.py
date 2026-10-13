from copy import deepcopy

import pytest

from retrycascade.followup_analysis import validate_trace


def fixture():
    window = {
        "start_us": 0,
        "end_us": 1_000_000,
        "queue_area_slot_us": 0,
        "stage_busy_worker_us": [10, 20, 30],
        "stage_available_worker_us": [1_000_000] * 3,
        "stage_consumed_work_milli": [1, 2, 3],
        "cohort": {
            "successes": 1,
            "failures": 0,
            "observed_outcomes": 1,
            "arrivals": 1,
            "right_censored": 0,
        },
        "cohort_deadline_abandoned": 0,
        "cohort_attempts_exhausted": 0,
        "cohort_budget_denied": 0,
        "logical_arrivals": 1,
        "unique_goodput": 1,
        "attempt_timeouts": 0,
        "pre_admission_failures": 0,
        "queue_rejections": 0,
        "late_completions": 0,
        "duplicate_completions": 0,
    }
    summary = {
        "counts": {"logical_arrivals": 1, "unique_goodput": 2},
        "v2_metrics": {
            "stage_busy_worker_us": [10, 20, 30],
            "stage_consumed_work_milli": [1, 2, 3],
            "recovery_trace": {"total_waiting_capacity": 100, "windows": [window]},
        },
    }
    return summary, {"observation": {"arrivals_begin_us": 0}, "run_end_us": 1_000_000}


def test_half_open_trace_allows_engine_event_at_exact_horizon():
    summary, config = fixture()
    # Top-level goodput includes a second completion exactly at run_end;
    # the [0,run_end) event bin correctly excludes that completion.
    validate_trace(summary, config)
    corrupted = deepcopy(summary)
    corrupted["v2_metrics"]["recovery_trace"]["windows"][0]["unique_goodput"] = 3
    with pytest.raises(ValueError, match="exceeds horizon"):
        validate_trace(corrupted, config)


def test_work_and_cohort_ledgers_remain_exact():
    summary, config = fixture()
    summary["v2_metrics"]["stage_consumed_work_milli"][0] += 1
    with pytest.raises(ValueError, match="does not conserve"):
        validate_trace(summary, config)
    summary, config = fixture()
    summary["v2_metrics"]["recovery_trace"]["windows"][0]["cohort"]["failures"] = 1
    with pytest.raises(ValueError, match="cohort outcomes"):
        validate_trace(summary, config)
