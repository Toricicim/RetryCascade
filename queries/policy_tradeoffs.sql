-- Point estimates within each operating condition; not a universal winner.
SELECT
    rho, failure_rate, trigger_seconds, arm, seeds,
    success_ratio, during_success_ratio, post_success_ratio,
    success_p95_ms, failure_p95_ms,
    work_units_per_request, attempts_per_request,
    recovery_count, recovery_observed, eligible_baselines,
    descriptive_mean_dominated_by
FROM cells
WHERE block = 'main'
ORDER BY rho, failure_rate, trigger_seconds, arm;
