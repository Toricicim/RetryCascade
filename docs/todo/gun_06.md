# Gün 6 TODO — Bounded Queue ve Sink Trigger

## Amaç

Queue taşmasını ve Sink kapasitesinin geçici düşüp geri gelmesini deterministik biçimde modellemek.

## Önkoşul

Retry'siz pipeline'ın elle hesaplı trace'i geçmeli.

## Must

- [ ] Her stage'de dolu waiting queue için anlık `queue_rejected(stage)` sonucu üret.
- [ ] Sink trigger'ını yarı-açık `[start, end)` aralığıyla uygula.
- [ ] Aktif işin kalan work miktarını rate değişiminde ceiling ile yeniden zamanla.
- [ ] Her reschedule için `service_epoch` artır; eski completion event'ini no-op yap.
- [ ] Trigger bitince queue ve inflight işleri temizleme.
- [ ] Occupancy değişimlerinde exact queue-area sayacını güncelle.

## Stretch

- [ ] 100 ms debug queue sample üret.
- [ ] Birden fazla trigger için yalnız veri modeli taslağı çıkar.

## Teslim

Bounded queue ve tek Sink capacity trigger'ı çalışan retry'siz model.

## Kabul testleri

- [ ] Dolu queue doğru stage'de rejection üretir.
- [ ] Trigger başlangıç/bitiş sınırında doğru kapasite kullanılır.
- [ ] Stale completion state'i değiştirmez.
- [ ] Multiplier `1.0`, no-trigger sonucu ile aynıdır.
- [ ] Elle hesaplı queue-area sonucu tutar.

## Stop gate

Trigger work-conservation veya epoch testi geçmiyorsa timeout/retry ekleme.

## Gün sonunda anlatabilmeliyim

Kapasite geri geldiğinde neden backlog otomatik silinmez ve stale event nedir?

## Sonraki güne devir

Debug sampling devredilebilir; remaining-work ve stale-event güvenliği devredilemez.
