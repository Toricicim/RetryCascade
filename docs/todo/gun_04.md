# Gün 4 TODO — Deterministik Scheduler ve Asgari RNG

## Amaç

Aynı config ve seed için olayları her zaman aynı sırada işleyen zaman motorunu kurmak.

## Önkoşul

Strict `ResolvedRunConfig` testleri geçmeli.

## Must

- [ ] `EventKind`, `Event` ve monoton `sequence_id` tiplerini yaz.
- [ ] `BinaryHeap` üzerinde en erken olayı önce çıkaran total ordering kur.
- [ ] Aynı tick priority sırasını sözleşmeye göre uygula.
- [ ] Olay zamanı eklemelerinde checked arithmetic kullan.
- [ ] Keyed RNG için seed/namespace API'sinin küçük iskeletini kur.
- [ ] Aynı seed'in aynı olay/RNG çıktısını verdiğini doğrula.

## Stretch

- [ ] Scheduler debug görünümü ekle.
- [ ] Büyük olay yığını benchmark'ı yaz.

## Teslim

State değiştirmeyen, deterministik olay kuyruğu ve küçük RNG sözleşmesi.

## Kabul testleri

- [ ] Yedi olay türü ters sırada eklenince doğru priority ile çıkar.
- [ ] Aynı priority'de `sequence_id` sırasını korur.
- [ ] Completion aynı tick'teki timeout'tan önce gelir.
- [ ] Zaman taşması hata üretir; boş scheduler güvenli davranır.
- [ ] Aynı seed iki çalıştırmada aynı golden sonucu verir.

## Stop gate

Aynı girdi aynı event/RNG sırasını üretmiyorsa pipeline'a geçme.

## Gün sonunda anlatabilmeliyim

Discrete-event simulation neden gerçek zamanla uyumaz ve `BinaryHeap` neden kullanılır?

## Sonraki güne devir

Performans benchmark'ı devredilebilir; deterministik sıralama devredilemez.
