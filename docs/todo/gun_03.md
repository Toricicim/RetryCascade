# Gün 3 TODO — Rust Tipleri ve Resolved Config

## Durum

`TAMAMLANDI`. Python'ın ürettiği resolved JSON artık Rust tarafından strict biçimde okunuyor ve semantik olarak doğrulanıyor.

## Amaç

Python'ın ürettiği tamamen çözülmüş tek-koşu JSON dosyasını Rust'ta güvenli biçimde okumak. Rust araştırma YAML'ını birleştirmeyecek ve hash üretmeyecek.

## Önkoşul

Gün 2 semantik ve araştırma protokolü tamamlandı.

## Python resolved-config ön hazırlığı

- [x] `research_protocol.yaml` ile tek koşu seçimini birleştiren `resolve_run_config` iskeletini oluştur.
- [x] Üç stage, workload, request semantiği ve retry sözleşmesini resolved çıktıya taşı.
- [x] Trigger başlangıç/bitiş, post-trigger duration ve `run_end_us` değerlerini çöz.
- [x] Seçilen policy için yalnız gerekli policy parametrelerini üret.
- [x] Observation, recovery, RNG ve event safety alanlarını resolved çıktıya taşı.
- [x] Dört policy'nin doğru ve yalnız kendine ait parametrelerle çözüldüğünü test et.
- [x] Resolver testlerini geçir: `9 passed`; Python test paketinin tamamı `28 passed`.
- [x] `protocol_sha256` ve `run_config_sha256` değerlerini resolved çıktıya ekle.
- [x] Tam resolved-run JSON Schema'yı oluştur ve resolved sözlüğü bu şemayla doğrula.
- [x] Deterministik golden `resolved_run_config.json` dosyasını üret ve hash testini ekle.

## Must

- [x] `Tick`, `LogicalRequestId`, `AttemptId` ve `StageId` newtype'larını yaz.
- [x] `ResolvedRunConfig` ve iç tiplerini `serde` ile tanımla.
- [x] Bilinmeyen ve eksik alanları reddet.
- [x] `thiserror` ile anlaşılır config/validation hataları oluştur.
- [x] Pozitif kapasite, süre, timeout/deadline ve üç stage invariant'larını doğrula.
- [x] Zaman toplama/çevirme işlemlerinde checked arithmetic kullan.
- [x] Python'ın verdiği protocol/run hash ve sürümleri çıktıya aynen taşı.

## Stretch

- [ ] Daha okunaklı hata konumu/path bilgisi ekle.
- [ ] Rust toolchain sürümünü sabitle.

## Teslim

Strict resolved JSON okuyup doğrulayan küçük Rust config katmanı.

## Kabul testleri

- [x] Geçerli golden resolved fixture parse edilir. Henüz tam resolved smoke fixture üretilmediği için smoke testi Gün 10'a bırakıldı.
- [x] Eksik sürüm veya bilinmeyen alan reddedilir.
- [x] Sıfır kapasite ve `timeout >= deadline` reddedilir.
- [x] Taşma oluşturacak zaman değeri kontrollü hata verir.
- [x] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` ve `cargo test --workspace --locked` geçer.

## Stop gate

Python resolved config şemadan geçmeden ve golden JSON üretilmeden Rust struct'larına başlama. Rust'ın kullandığı herhangi bir bilimsel parametre resolved JSON içinde görünmüyorsa Gün 4'e geçme.

## Gün sonunda anlatabilmeliyim

`struct`, `enum`, newtype, `Option`, `Result` ve `serde` burada ne işe yarıyor?

## Sonraki güne devir

Yalnız hata mesajı iyileştirmeleri devredilebilir; config sözleşmesi devredilemez.
