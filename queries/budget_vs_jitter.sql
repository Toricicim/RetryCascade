-- Paired seed differences and exploratory bootstrap intervals.
-- success_ratio: positive favours budget; work/latency: negative favours budget.
SELECT
    rho, failure_rate, trigger_seconds, arm, metric,
    pairs, planned_pairs, mean_difference, ci_low, ci_high
FROM paired_effects
WHERE arm LIKE 'budget_%' AND block = 'main'
ORDER BY rho, failure_rate, trigger_seconds, arm, metric;
