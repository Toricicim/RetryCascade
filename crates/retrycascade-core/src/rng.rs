use rand_chacha::ChaCha20Rng;
use rand_core::{RngCore, SeedableRng};
use sha2::{Digest, Sha256};

use crate::types::{LogicalRequestId, StageId};

const RNG_VERSION: &str = "keyed-chacha20-v1";
const ARRIVAL_NAMESPACE: &str = "external_arrival";
const SERVICE_NAMESPACE: &str = "service_demand";
const JITTER_NAMESPACE: &str = "retry_jitter";

/// Her örneği ana seed ve olay kimliğinden bağımsız olarak yeniden üretir.
/// Çağrı sırası sonucu değiştirmez; aynı anahtar her zaman aynı sayıyı verir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyedRng {
    main_seed: u64,
}

impl KeyedRng {
    pub const fn new(main_seed: u64) -> Self {
        Self { main_seed }
    }

    pub fn arrival_u64(self, logical_arrival_index: u64) -> u64 {
        self.sample(
            ARRIVAL_NAMESPACE,
            &[KeyPart::Unsigned(logical_arrival_index)],
        )
    }

    pub fn service_u64(
        self,
        logical_request_id: LogicalRequestId,
        attempt_no: u64,
        stage_id: StageId,
    ) -> u64 {
        self.sample(
            SERVICE_NAMESPACE,
            &[
                KeyPart::Unsigned(logical_request_id.0),
                KeyPart::Unsigned(attempt_no),
                KeyPart::Unsigned(u64::from(stage_id.0)),
            ],
        )
    }

    pub fn jitter_u64(
        self,
        logical_request_id: LogicalRequestId,
        attempt_no: u64,
        policy_name: &str,
    ) -> u64 {
        self.sample(
            JITTER_NAMESPACE,
            &[
                KeyPart::Unsigned(logical_request_id.0),
                KeyPart::Unsigned(attempt_no),
                KeyPart::Text(policy_name),
            ],
        )
    }

    fn sample(self, namespace: &str, parts: &[KeyPart<'_>]) -> u64 {
        let mut hasher = Sha256::new();
        encode_text(&mut hasher, RNG_VERSION);
        encode_unsigned(&mut hasher, self.main_seed);
        encode_text(&mut hasher, namespace);

        for part in parts {
            match part {
                KeyPart::Unsigned(value) => {
                    hasher.update([0]);
                    encode_unsigned(&mut hasher, *value);
                }
                KeyPart::Text(value) => {
                    hasher.update([1]);
                    encode_text(&mut hasher, value);
                }
            }
        }

        let seed: [u8; 32] = hasher.finalize().into();
        let mut rng = ChaCha20Rng::from_seed(seed);
        rng.next_u64()
    }
}

enum KeyPart<'a> {
    Unsigned(u64),
    Text(&'a str),
}

fn encode_unsigned(hasher: &mut Sha256, value: u64) {
    hasher.update(value.to_le_bytes());
}

fn encode_text(hasher: &mut Sha256, value: &str) {
    let bytes = value.as_bytes();
    encode_unsigned(hasher, bytes.len() as u64);
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::KeyedRng;
    use crate::types::{LogicalRequestId, StageId};

    #[test]
    fn same_seed_and_key_produce_the_same_value() {
        let first = KeyedRng::new(1001).arrival_u64(42);
        let second = KeyedRng::new(1001).arrival_u64(42);

        assert_eq!(first, second);
    }

    #[test]
    fn namespaces_and_key_parts_produce_separate_streams() {
        let rng = KeyedRng::new(1001);
        let arrival = rng.arrival_u64(42);
        let service = rng.service_u64(LogicalRequestId(42), 0, StageId(0));
        let jitter = rng.jitter_u64(LogicalRequestId(42), 0, "full_jitter");

        assert_ne!(arrival, service);
        assert_ne!(arrival, jitter);
        assert_ne!(service, jitter);
    }

    #[test]
    fn rng_v1_matches_the_frozen_golden_vector() {
        let rng = KeyedRng::new(1001);
        let values = (
            rng.arrival_u64(42),
            rng.service_u64(LogicalRequestId(42), 0, StageId(0)),
            rng.jitter_u64(LogicalRequestId(42), 0, "full_jitter"),
        );

        assert_eq!(
            values,
            (
                15_440_190_499_650_710_418,
                9_063_676_014_666_612_407,
                17_263_770_914_295_489_527,
            )
        );
    }
}
