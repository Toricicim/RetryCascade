-- Paired per-seed structural sensitivity; not a claim that strict RPC is wrong.
SELECT variant.arm, variant.seed, variant.client_contract,
       variant.during_reference_success_ratio - strict.during_reference_success_ratio
           AS incident_success_difference,
       variant.early_post_success_ratio - strict.early_post_success_ratio
           AS early_post_success_difference,
       variant.work_units_per_request - strict.work_units_per_request
           AS work_difference
FROM runs variant JOIN runs strict
  ON variant.rho = strict.rho
 AND variant.failure_rate = strict.failure_rate
 AND variant.trigger_seconds = strict.trigger_seconds
 AND variant.reference_trigger_seconds = strict.reference_trigger_seconds
 AND variant.timeout_us = strict.timeout_us
 AND variant.demand_identity = strict.demand_identity
 AND variant.arm = strict.arm AND variant.seed = strict.seed
WHERE variant.client_contract <> 'strict_rpc'
  AND strict.client_contract = 'strict_rpc'
  AND variant.trigger_seconds > 0
ORDER BY variant.arm, variant.client_contract, variant.seed;
