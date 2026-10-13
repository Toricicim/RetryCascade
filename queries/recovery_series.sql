-- Completed-bin features, not predictions or mature arrival-cohort outcomes.
SELECT arm, seed, seconds_since_reference_trigger_end,
       descriptive_recovery_index, goodput_ratio_to_baseline,
       mean_waiting_slots, goodput_delta_per_second_squared,
       goodput_deficit_auc_ratio_seconds
FROM recovery_features
WHERE rho = 0.65 AND failure_rate = 0.1 AND trigger_seconds = 10
  AND client_contract = 'strict_rpc'
ORDER BY arm, seed, end_us;
