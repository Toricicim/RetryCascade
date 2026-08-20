use std::path::PathBuf;

use retrycascade_core::config::{
    ConfigError, PolicyConfig, StageName, load_resolved_run_config, parse_resolved_run_config,
};
use retrycascade_core::validation::ValidationError;
use serde_json::{Value, json};

const GOLDEN: &str = include_str!("../../../tests/golden/resolved_run_config.json");
const SMOKE_GOLDEN: &str = include_str!("../../../tests/golden/resolved_smoke_config.json");

fn golden_value() -> Value {
    serde_json::from_str(GOLDEN).expect("the checked-in golden fixture must be JSON")
}

fn parse_value(value: Value) -> Result<retrycascade_core::config::ResolvedRunConfig, ConfigError> {
    parse_resolved_run_config(
        &serde_json::to_string(&value).expect("the test value must serialize"),
    )
}

#[test]
fn accepts_the_golden_resolved_config() {
    let config = parse_resolved_run_config(GOLDEN).expect("golden config must be valid");

    assert_eq!(config.rho_basis_points, 9_000);
    assert_eq!(config.stages[0].name, StageName::Source);
    assert_eq!(config.stages[1].name, StageName::Transform);
    assert_eq!(config.stages[2].name, StageName::Sink);
    assert!(matches!(config.policy, PolicyConfig::Immediate { .. }));
}

#[test]
fn accepts_the_small_non_analysis_smoke_config() {
    let config = parse_resolved_run_config(SMOKE_GOLDEN).expect("smoke config must be valid");

    assert!(!config.analysis_eligible);
    assert_eq!(config.run_end_us.as_u64(), 6_000_000);
    assert_eq!(config.maximum_events, 100_000);
    assert!(
        config
            .stages
            .iter()
            .all(|stage| stage.queue_capacity_waiting == 8)
    );
}

#[test]
fn rejects_unknown_top_level_and_nested_fields() {
    let mut top_level = golden_value();
    top_level["surprise"] = json!(true);
    assert!(matches!(parse_value(top_level), Err(ConfigError::Json(_))));

    let mut nested = golden_value();
    nested["observation"]["surprise"] = json!(true);
    assert!(matches!(parse_value(nested), Err(ConfigError::Json(_))));
}

#[test]
fn rejects_missing_wrongly_typed_and_unknown_enum_fields() {
    let mut missing = golden_value();
    missing
        .as_object_mut()
        .expect("golden root is an object")
        .remove("schema_version");
    assert!(matches!(parse_value(missing), Err(ConfigError::Json(_))));

    let mut wrong_type = golden_value();
    wrong_type["seed"] = json!("1001");
    assert!(matches!(parse_value(wrong_type), Err(ConfigError::Json(_))));

    let mut unknown_policy = golden_value();
    unknown_policy["policy"] = json!({"kind": "magic_retry"});
    assert!(matches!(
        parse_value(unknown_policy),
        Err(ConfigError::Json(_))
    ));
}

#[test]
fn accepts_all_four_policy_shapes() {
    let policies = [
        json!({"kind": "immediate", "epsilon_us": 1}),
        json!({"kind": "exponential", "base_us": 25_000, "cap_us": 200_000}),
        json!({
            "kind": "full_jitter",
            "base_us": 25_000,
            "cap_us": 200_000,
            "rng_namespace": "retry_jitter"
        }),
        json!({
            "kind": "retry_budget",
            "base_us": 25_000,
            "cap_us": 200_000,
            "token_bucket": {
                "units_per_token": 1,
                "retry_cost_units": 5,
                "refill_units_per_unique_on_time_success": 1,
                "capacity_units": 100,
                "initial_units": 100,
                "token_denial_result": "retry_denied"
            }
        }),
    ];

    for policy in policies {
        let mut value = golden_value();
        value["policy"] = policy;
        parse_value(value).expect("each frozen policy shape must be valid");
    }
}

#[test]
fn rejects_fields_borrowed_from_another_policy_shape() {
    let mut value = golden_value();
    value["policy"]["base_us"] = json!(25_000);

    assert!(matches!(parse_value(value), Err(ConfigError::Json(_))));
}

#[test]
fn rejects_invalid_stage_and_retry_invariants() {
    let mut zero_workers = golden_value();
    zero_workers["stages"][0]["workers"] = json!(0);
    assert!(matches!(
        parse_value(zero_workers),
        Err(ConfigError::Validation(ValidationError::OutOfRange { .. }))
    ));

    let mut wrong_order = golden_value();
    wrong_order["stages"][0]["name"] = json!("transform");
    wrong_order["stages"][1]["name"] = json!("source");
    assert!(matches!(
        parse_value(wrong_order),
        Err(ConfigError::Validation(
            ValidationError::InvalidStageOrder { .. }
        ))
    ));

    let mut sink_not_bottleneck = golden_value();
    sink_not_bottleneck["stages"][2]["nominal_rate_work_units_per_second"] = json!(200);
    assert!(matches!(
        parse_value(sink_not_bottleneck),
        Err(ConfigError::Validation(
            ValidationError::SinkIsNotBottleneck { .. }
        ))
    ));

    let mut timeout_equals_deadline = golden_value();
    timeout_equals_deadline["retry_contract"]["per_attempt_timeout_us"] = json!(2_000_000);
    assert!(matches!(
        parse_value(timeout_equals_deadline),
        Err(ConfigError::Validation(
            ValidationError::InvalidOrdering { .. }
        ))
    ));
}

#[test]
fn rejects_invalid_policy_invariants() {
    let mut reversed_backoff = golden_value();
    reversed_backoff["policy"] =
        json!({"kind": "exponential", "base_us": 300_000, "cap_us": 200_000});
    assert!(matches!(
        parse_value(reversed_backoff),
        Err(ConfigError::Validation(ValidationError::OutOfRange { .. }))
    ));

    let mut excessive_initial_budget = golden_value();
    excessive_initial_budget["policy"] = json!({
        "kind": "retry_budget",
        "base_us": 25_000,
        "cap_us": 200_000,
        "token_bucket": {
            "units_per_token": 1,
            "retry_cost_units": 5,
            "refill_units_per_unique_on_time_success": 1,
            "capacity_units": 100,
            "initial_units": 101,
            "token_denial_result": "retry_denied"
        }
    });
    assert!(matches!(
        parse_value(excessive_initial_budget),
        Err(ConfigError::Validation(ValidationError::OutOfRange { .. }))
    ));
}

#[test]
fn rejects_inconsistent_and_overflowing_derived_times() {
    let mut inconsistent = golden_value();
    inconsistent["trigger_end_us"] = json!(311_000_000_u64);
    assert!(matches!(
        parse_value(inconsistent),
        Err(ConfigError::Validation(
            ValidationError::InconsistentDerivedTime { .. }
        ))
    ));

    let mut overflowing = golden_value();
    overflowing["trigger_start_us"] = json!(u64::MAX);
    overflowing["trigger_duration_us"] = json!(1);
    overflowing["trigger_end_us"] = json!(u64::MAX);
    overflowing["observation"]["baseline_end_us"] = json!(u64::MAX);
    assert!(matches!(
        parse_value(overflowing),
        Err(ConfigError::Validation(ValidationError::TimeOverflow {
            field: "trigger_end_us"
        }))
    ));
}

#[test]
fn rejects_invalid_observation_order() {
    let mut value = golden_value();
    value["observation"]["baseline_start_us"] = json!(301_000_000_u64);

    assert!(matches!(
        parse_value(value),
        Err(ConfigError::Validation(
            ValidationError::InvalidOrdering { .. }
        ))
    ));
}

#[test]
fn copies_provenance_without_recomputing_python_hashes() {
    let mut value = golden_value();
    value["seed"] = json!(42);
    let expected_protocol_hash = value["protocol_sha256"]
        .as_str()
        .expect("hash is a string")
        .to_owned();
    let expected_run_hash = value["run_config_sha256"]
        .as_str()
        .expect("hash is a string")
        .to_owned();

    let config = parse_value(value).expect("Rust validates format but does not recompute hashes");
    let provenance = config.provenance();

    assert_eq!(provenance.seed, 42);
    assert_eq!(provenance.protocol_sha256, expected_protocol_hash);
    assert_eq!(provenance.run_config_sha256, expected_run_hash);
}

#[test]
fn reports_a_missing_config_file_as_a_read_error() {
    let path = PathBuf::from("this-file-does-not-exist.json");

    assert!(matches!(
        load_resolved_run_config(path),
        Err(ConfigError::Read { .. })
    ));
}
