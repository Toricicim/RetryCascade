# Gün 3 TODO — Rust Tipleri ve Resolved Config

## Amaç

Python'ın ürettiği tamamen çözülmüş tek-koşu JSON dosyasını Rust'ta güvenli biçimde okumak. Rust araştırma YAML'ını birleştirmeyecek ve hash üretmeyecek.

## Önkoşul

Gün 2 hard gate tamamen kapanmış olmalı.

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

Rust'ın kullandığı herhangi bir bilimsel parametre resolved JSON içinde görünmüyorsa Gün 4'e geçme.

## Gün sonunda anlatabilmeliyim

`struct`, `enum`, newtype, `Option`, `Result` ve `serde` burada ne işe yarıyor?

## Sonraki güne devir

Yalnız hata mesajı iyileştirmeleri devredilebilir; config sözleşmesi devredilemez.
