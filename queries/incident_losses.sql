-- Positive excess failure means worse user outcomes than a matched control.
SELECT rho, failure_rate, trigger_seconds, arm, client_contract,
       during_reference_success_ratio, early_post_success_ratio,
       matched_incident_excess_failure_ratio, failure_p95_ms,
       budget_denied_ratio, recovery_count, recovery_observed
FROM cells
WHERE trigger_seconds > 0
ORDER BY rho, failure_rate, trigger_seconds, client_contract, arm;
