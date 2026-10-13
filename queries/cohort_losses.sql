-- Overall success can conceal the requests arriving during the capacity loss.
SELECT rho, failure_rate, trigger_seconds, arm,
       success_ratio, during_success_ratio, post_success_ratio,
       budget_denied_ratio
FROM cells
WHERE block = 'main' AND trigger_seconds > 0
ORDER BY rho, failure_rate, trigger_seconds, arm;
