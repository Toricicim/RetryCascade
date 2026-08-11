from copy import deepcopy
from pathlib import Path

import pytest

from retrycascade.protocol import (
    ProtocolError,
    canonical_sha256,
    load_and_validate,
    validate_protocol_semantics,
)


ROOT = Path(__file__).resolve().parents[2]
PROTOCOL = ROOT / "configs" / "research_protocol.yaml"
PROTOCOL_SCHEMA = ROOT / "schemas" / "research-protocol.schema.json"
RUN_SCHEMA = ROOT / "schemas" / "run-config.schema.json"


def test_research_protocol_is_valid_and_hash_is_stable() -> None:
    protocol = load_and_validate(PROTOCOL, PROTOCOL_SCHEMA, kind="protocol")
    digest = canonical_sha256(protocol)
    expected = (ROOT / "configs" / "research_protocol.sha256").read_text(encoding="utf-8").strip()
    assert digest == canonical_sha256(deepcopy(protocol))
    assert digest == expected


@pytest.mark.parametrize("name", ["smoke.yaml", "canonical_metastable.yaml"])
def test_run_examples_are_valid(name: str) -> None:
    load_and_validate(ROOT / "configs" / name, RUN_SCHEMA, kind="run")


def test_scientific_parameter_change_changes_hash() -> None:
    protocol = load_and_validate(PROTOCOL, PROTOCOL_SCHEMA, kind="protocol")
    changed = deepcopy(protocol)
    changed["retry_contract"]["per_attempt_timeout_us"] += 1
    assert canonical_sha256(changed) != canonical_sha256(protocol)


def test_invalid_rho_is_rejected() -> None:
    protocol = load_and_validate(PROTOCOL, PROTOCOL_SCHEMA, kind="protocol")
    protocol["workload"]["arrival_process"]["rho_basis_points"][-1] = 10_000
    with pytest.raises(ProtocolError, match="rho_basis_points"):
        validate_protocol_semantics(protocol)


def test_timeout_must_be_less_than_deadline() -> None:
    protocol = load_and_validate(PROTOCOL, PROTOCOL_SCHEMA, kind="protocol")
    protocol["retry_contract"]["per_attempt_timeout_us"] = protocol["retry_contract"][
        "overall_logical_deadline_us"
    ]
    with pytest.raises(ProtocolError, match="timeout"):
        validate_protocol_semantics(protocol)


def test_factor_matrix_counts_are_checked() -> None:
    protocol = load_and_validate(PROTOCOL, PROTOCOL_SCHEMA, kind="protocol")
    protocol["experiment"]["expected_total_primary_runs"] = 2999
    with pytest.raises(ProtocolError, match="expected_total_primary_runs"):
        validate_protocol_semantics(protocol)


def test_float_is_forbidden_in_canonical_domain() -> None:
    protocol = load_and_validate(PROTOCOL, PROTOCOL_SCHEMA, kind="protocol")
    protocol["trigger"]["capacity_multiplier_basis_points"] = 0.5
    with pytest.raises(ProtocolError, match="Floating-point"):
        validate_protocol_semantics(protocol)
