# Gün 3 TODO — Rust Tipleri ve Resolved Config

## Durum

`DEVAM EDİYOR — PYTHON → JSON GEÇİŞİ TAMAM`. Sıradaki iş Rust config tipleri.

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

- [ ] `Tick`, `LogicalRequestId`, `AttemptId` ve `StageId` newtype'larını yaz.
- [ ] `ResolvedRunConfig` ve iç tiplerini `serde` ile tanımla.
- [ ] Bilinmeyen ve eksik alanları reddet.
- [ ] `thiserror` ile anlaşılır config/validation hataları oluştur.
- [ ] Pozitif kapasite, süre, timeout/deadline ve üç stage invariant'larını doğrula.
- [ ] Zaman toplama/çevirme işlemlerinde checked arithmetic kullan.
- [ ] Python'ın verdiği protocol/run hash ve sürümleri çıktıya aynen taşı.

## Stretch

- [ ] Daha okunaklı hata konumu/path bilgisi ekle.
- [ ] Rust toolchain sürümünü sabitle.

## Teslim

Strict resolved JSON okuyup doğrulayan küçük Rust config katmanı.

## Kabul testleri

- [ ] Geçerli smoke fixture parse edilir.
- [ ] Eksik sürüm veya bilinmeyen alan reddedilir.
- [ ] Sıfır kapasite ve `timeout >= deadline` reddedilir.
- [ ] Taşma oluşturacak zaman değeri kontrollü hata verir.
- [ ] `cargo fmt --check`, `cargo clippy --workspace -- -D warnings` ve `cargo test --workspace` geçer.

## Stop gate

Python resolved config şemadan geçmeden ve golden JSON üretilmeden Rust struct'larına başlama. Rust'ın kullandığı herhangi bir bilimsel parametre resolved JSON içinde görünmüyorsa Gün 4'e geçme.

## Gün sonunda anlatabilmeliyim

`struct`, `enum`, newtype, `Option`, `Result` ve `serde` burada ne işe yarıyor?

## Sonraki güne devir

Yalnız hata mesajı iyileştirmeleri devredilebilir; config sözleşmesi devredilemez.
