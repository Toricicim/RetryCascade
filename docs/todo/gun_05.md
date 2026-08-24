# Gün 5 TODO — Retry'siz Pipeline

## Amaç

Bir isteği retry ve trigger olmadan `Source → Transform → Sink` hattından geçirmek.

## Önkoşul

Scheduler sıralama ve tekrar üretilebilirlik testleri geçmeli.

## Must

- [x] Önce tek stage ile worker + FIFO waiting queue mantığını kur.
- [ ] Sonra aynı generic `Stage` tipini üç aşamaya genişlet.
- [x] Waiting queue ile in-service iş sayısını ayrı tut.
- [ ] Sabit service-demand fixture ile ilk dikey akışı çalıştır.
- [ ] Stage completion sonrasında sıradaki stage'e admission yap.
- [ ] Temel arrivals/completions/occupancy sayaçlarını ekle.

## Stretch

- [ ] Lognormal service-demand örneklemesini bağla.
- [ ] Debug trace'i küçük NDJSON olarak üret.

## Teslim

Retry'siz ve trigger'sız çalışan generic üç-aşamalı pipeline.

## Kabul testleri

- [ ] Tek request'in üç completion zamanı elle yapılan hesapla aynıdır.
- [x] Üç request FIFO sırasını korur.
- [ ] Waiting occupancy `0..queue_capacity`, çalışan sayısı `0..workers` aralığındadır.
- [ ] Aynı fixture aynı summary/checksum'u verir.

## Stop gate

Elle hesaplanan trace veya worker/queue conservation tutmuyorsa trigger ekleme.

## Gün sonunda anlatabilmeliyim

Worker, waiting queue, service demand ve service rate arasındaki fark nedir?

## Sonraki güne devir

Lognormal dağılım devredilebilir; retry'siz üç-stage akışı devredilemez.
