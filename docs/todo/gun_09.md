# Gün 9 TODO — Full Jitter ve Retry Budget

## Amaç

Retry zamanlarını rastgele dağıtan ve toplam retry miktarını sınırlayan iki politikayı eklemek.

## Önkoşul

Immediate ve exponential policy contract testleri geçmeli.

## Must

- [ ] Full jitter'ı exponential ile aynı base/cap schedule üzerinde uygula.
- [ ] Jitter için policy-specific keyed RNG namespace kullan.
- [ ] Run-global retry token bucket oluştur.
- [ ] İlk attempt'in token istememesini sağla.
- [ ] Token yoksa retry'ı reddet ve sayaca yaz.
- [ ] Yalnız ilk unique on-time success ile refill yap.
- [ ] Token sayısını her zaman `0..capacity` aralığında tut.
- [ ] Dört policy'yi aynı lifecycle contract testlerinden geçir.

## Stretch

- [ ] Source load shedding'i ayrı ikincil mitigation olarak uygula.
- [ ] Jitter dağılımı için istatistiksel uniformity testi yaz.

## Teslim

Dört retry policy'si çalışan ve ortak bilimsel sözleşmeye uyan çekirdek.

## Kabul testleri

- [ ] Jitter örneği her zaman izin verilen aralıktadır ve seed ile tekrarlanır.
- [ ] Budget token'ı negatif veya kapasite üstü olmaz.
- [ ] Denied retry ve refill sayaçları elle hesaplı fixture ile tutar.
- [ ] Dört policy'de ortak arrival/service trace hash'leri aynıdır.
- [ ] Dört policy'de lifecycle ledger kapanır.

## Stop gate

Budget ile shedding birbirine karışırsa veya ortak trace eşitliği bozulursa metrik aşamasına geçme.

## Gün sonunda anlatabilmeliyim

Jitter ile retry budget aynı sorunu neden iki farklı yoldan azaltır?

## Sonraki güne devir

Source shedding ve uniformity testi devredilir; dört ana policy devredilemez.
