# Gün 10 TODO — Metrikler, Recovery ve Uçtan Uca Smoke

## Amaç

Simülasyonun ham olaylarını bilimsel olarak tanımlı özetlere dönüştürmek ve küçük uçtan uca koşuyu doğrulamak.

## Önkoşul

Dört policy aynı lifecycle ve ortak-trace sözleşmesini geçmeli.

## Must

- [ ] Logical arrivals, physical attempts, unique goodput, late, duplicate, rejection ve outstanding sayaçlarını üret.
- [ ] Retry amplification'ı açık pay/payda ve zaman penceresiyle hesapla.
- [ ] Queue değişimlerinden exact time-weighted mean waiting occupancy hesapla.
- [ ] Trigger öncesi baseline `G0/Q0` değerlerini sözleşmeye göre çıkar.
- [ ] Trigger-end hizalı üç ardışık 30 saniye recovery penceresini uygula.
- [ ] Başarısız pencerenin recovery streak'ini sıfırlamasını sağla.
- [ ] `H=300/600/900` için recovery/right-censoring sonucunu temsil et.
- [ ] Typed ve sürümlü `run_summary.json` üret.
- [ ] CLI ile resolved smoke config'ten summary'ye uçtan uca koşu yap.

## Stretch

- [ ] Exact time-weighted p95 veya queue AUC ekle.
- [ ] Peak amplification optimizasyonu yap.
- [ ] Manifest schema taslağı çıkar; writer'ı Gün 11'e bırak.

## Teslim

`resolved config → deterministic simulation → closed ledgers → run_summary.json` zinciri.

## Kabul testleri

- [ ] Elle hazırlanmış trace'in bütün sayaçları doğru çıkar.
- [ ] Üç iyi recovery penceresi geçer; aradaki kötü pencere streak'i bozar.
- [ ] Horizon sınırındaki recovery/outstanding sınıflaması doğrudur.
- [ ] Debug sample interval değişince bilimsel queue özeti değişmez.
- [ ] Aynı config/seed aynı summary checksum'u üretir.
- [ ] CLI smoke exit code `0`; invariant ihlali non-zero döner.

## Stop gate

Resolved config'ten deterministik run'a, kapanan ledger'a, dört policy'ye ve doğru recovery summary'ye uzanan zincir çalışmıyorsa Gün 11 artifact katmanına geçme.

## Gün sonunda anlatabilmeliyim

Goodput, attempt amplification, time-weighted queue ve operational non-recovery neyi ölçüyor; neyi kanıtlamıyor?

## Sonraki güne devir

P95/AUC/peak metrikleri devredilebilir. Bilimsel summary ve recovery sınıflaması devredilemez.
