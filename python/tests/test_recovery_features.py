import pytest

from retrycascade.recovery_features import (
    derive_recovery_features,
    independent_failure_nulls,
    stationary_budget_reference,
)

BASELINE = {"eligible": True, "g0_per_second": 10, "q0_slots": 0}
SPEC = {
    "queue_capacity_waiting": 100,
    "queue_tolerance_slots": 5,
    "rolling_window_us": 3_000_000,
}


def bin_row(second, goodput=10, queue=0, full=True):
    return {
        "start_us": second * 1_000_000,
        "end_us": (second + 1) * 1_000_000,
        "unique_goodput": goodput,
        "queue_area_slot_us": queue * 1_000_000,
        "is_full_window": full,
        "cohort": {"success_ratio": 0.0},
    }


def test_index_endpoints_overshoot_and_user_success_are_distinct():
    rows = derive_recovery_features(
        [bin_row(0, 0, 100), bin_row(1, 20, 0)], BASELINE, SPEC
    )
    assert rows[0]["descriptive_recovery_index"] == 0
    assert rows[1]["descriptive_recovery_index"] == 100
    assert rows[1]["goodput_ratio_to_baseline"] == 2
    assert (
        "cohort" not in rows[1]
    )  # Retrospective cohort outcomes are never live features.


def test_features_use_only_completed_past_bins():
    prefix = [bin_row(0, 2, 50), bin_row(1, 5, 25), bin_row(2, 10, 0)]
    first = derive_recovery_features(prefix, BASELINE, SPEC)
    extended = derive_recovery_features(
        prefix + [bin_row(3, 1000, 100)], BASELINE, SPEC
    )
    assert first == extended[:3]
    assert first[0]["rolling_goodput_per_second"] is None
    assert first[2]["rolling_goodput_per_second"] == pytest.approx(17 / 3)
    assert first[2]["goodput_deficit_auc_ratio_seconds"] == pytest.approx(1.3)
    assert first[2]["queue_excess_auc_capacity_seconds"] == pytest.approx(0.75)


@pytest.mark.parametrize(
    "baseline",
    [
        {"eligible": False, "g0_per_second": 10, "q0_slots": 0},
        {"eligible": True, "g0_per_second": 0, "q0_slots": 0},
        {"eligible": True, "g0_per_second": None, "q0_slots": None},
    ],
)
def test_missing_or_invalid_baseline_is_unknown_not_failure(baseline):
    row = derive_recovery_features([bin_row(0)], baseline, SPEC)[0]
    assert row["event_goodput_per_second"] == 10
    assert row["descriptive_recovery_index"] is None
    assert row["goodput_deficit_auc_ratio_seconds"] is None


def test_reference_availability_and_full_window_requirement():
    baseline = {**BASELINE, "available_at_us": 2_000_000}
    rows = derive_recovery_features([bin_row(n) for n in range(5)], baseline, SPEC)
    assert rows[0]["descriptive_recovery_index"] is None
    assert rows[1]["descriptive_recovery_index"] == 100
    assert rows[2]["rolling_targets_pass"] is None
    assert rows[4]["rolling_targets_pass"] is True
    partial = bin_row(2, full=False)
    partial["end_us"] = 2_500_000
    partial["unique_goodput"] = 5
    rows = derive_recovery_features([bin_row(0), bin_row(1), partial], BASELINE, SPEC)
    assert rows[-1]["rolling_targets_pass"] is None


def test_queue_margin_is_explicit_capacity_fraction_not_literal_slots():
    row = derive_recovery_features([bin_row(0, 10, 5)], BASELINE, SPEC)[0]
    assert row["descriptive_recovery_index"] == 100
    row = derive_recovery_features([bin_row(0, 10, 6)], BASELINE, SPEC)[0]
    assert row["descriptive_recovery_index"] < 100


def test_boundary_split_bins_can_form_exact_rolling_window():
    first, second = bin_row(0, full=False), bin_row(0, full=False)
    first["end_us"], first["unique_goodput"] = 500_000, 5
    second["start_us"], second["unique_goodput"] = 500_000, 5
    rows = derive_recovery_features(
        [first, second], BASELINE, {**SPEC, "rolling_window_us": 1_000_000}
    )
    assert rows[0]["rolling_targets_pass"] is None
    assert rows[1]["rolling_targets_pass"] is True
    assert rows[1]["rolling_goodput_per_second"] == 10


def test_series_rejects_out_of_order_gaps_and_invalid_queue_area():
    for series in (
        [bin_row(1), bin_row(0)],
        [bin_row(0), bin_row(2)],
        [bin_row(0, queue=101)],
    ):
        with pytest.raises(ValueError):
            derive_recovery_features(series, BASELINE, SPEC)


def test_iid_null_is_not_finite_budget_or_queue_prediction():
    reference = independent_failure_nulls(0.2)
    assert reference["no_retry_success"] == pytest.approx(0.8)
    assert reference["unlimited_budget_max_attempts_success"] == pytest.approx(0.992)
    assert reference["expected_attempts_without_deadlines"] == pytest.approx(1.24)
    assert "binding retry budget" in reference["assumptions"]


def test_stationary_budget_reference_is_explicit_bound_not_exact_prediction():
    bound = stationary_budget_reference(0.2, 0.1)
    assert bound["stationary_iid_success_upper_bound"] == pytest.approx(0.8 / 0.92)
    assert bound["token_constraint_binding_in_mean_field"] is True
    loose = stationary_budget_reference(0.2, 0.25)
    assert loose["stationary_iid_success_upper_bound"] == pytest.approx(0.992)
    assert stationary_budget_reference(0, 0)["stationary_iid_success_upper_bound"] == 1
    assert (
        stationary_budget_reference(1, 0.1)["stationary_iid_success_upper_bound"] == 0
    )
    assert "not an exact finite-run prediction" in bound["assumptions"]
