use serde::{Deserialize, Serialize};

/// Simülasyonun başlangıcından itibaren geçen mikrosaniye sayısı.
///
/// JSON'da normal bir pozitif tam sayı olarak görünür; Rust içinde ise
/// request veya attempt kimlikleriyle yanlışlıkla karıştırılamaz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Tick(u64);

impl Tick {
    pub const ZERO: Self = Self(0);

    pub const fn new(microseconds: u64) -> Self {
        Self(microseconds)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, duration_us: u64) -> Option<Self> {
        self.0.checked_add(duration_us).map(Self)
    }
}

/// Kullanıcının istediği tek yararlı işin kimliği.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalRequestId(pub u64);

/// Bir logical request'i tamamlamak için yapılan fiziksel denemenin kimliği.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttemptId(pub u64);

/// Source, Transform veya Sink aşamasının sayısal kimliği.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StageId(pub u8);

#[cfg(test)]
mod tests {
    use super::Tick;

    #[test]
    fn tick_checked_add_returns_the_sum() {
        let start = Tick::new(100);

        assert_eq!(start.checked_add(25), Some(Tick::new(125)));
    }

    #[test]
    fn tick_checked_add_rejects_overflow() {
        let almost_largest_tick = Tick::new(u64::MAX);

        assert_eq!(almost_largest_tick.checked_add(1), None);
    }
}
