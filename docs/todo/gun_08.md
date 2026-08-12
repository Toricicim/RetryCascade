# Gün 8 TODO — Immediate ve Exponential Retry

## Amaç

İki retry politikasını aynı lifecycle sözleşmesi üzerinde, adil ve döngüsüz biçimde uygulamak.

## Önkoşul

Lifecycle ledger bütün senaryolarda kapanmalı.

## Must

- [ ] Saf bir `retry_decision(context)` arayüzü oluştur.
- [ ] Immediate retry'ı aynı-tick döngüsünü önleyen epsilon ile uygula.
- [ ] Capped exponential gecikmesini `base, 2×base, ... cap` olarak uygula.
- [ ] `max_attempts` değerine ilk attempt'i dahil et.
- [ ] Timeout ve queue rejection için attempt başına çift retry planlanmasını engelle.
- [ ] RetryReady anında success, deadline ve max-attempt koşullarını yeniden kontrol et.
- [ ] Policy değişse de external arrival/service-demand trace namespace'lerini sabit tut.

## Stretch

- [ ] Tüm policy'ler için ortak contract-test yardımcı fonksiyonu yaz.

## Teslim

Immediate ve exponential politikalarıyla deterministik retry akışı.

## Kabul testleri

- [ ] Immediate retry `now + epsilon` zamanına planlanır.
- [ ] Exponential gecikme tablosu cap'te doğru durur ve taşmaz.
- [ ] Deadline sonrası ve `max_attempts` üstünde retry oluşmaz.
- [ ] Aynı cause için en fazla bir retry planlanır.
- [ ] Event cap/livelock oluşmaz.

## Stop gate

Policy değişimi ortak arrival/service trace'ini değiştirirse veya ledger kapanmazsa Gün 9'a geçme.

## Gün sonunda anlatabilmeliyim

Exponential backoff neden yükü zamana yayar; immediate retry nasıl amplification üretir?

## Sonraki güne devir

Contract helper devredilebilir; deadline/max-attempt güvenliği devredilemez.
