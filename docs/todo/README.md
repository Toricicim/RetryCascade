# RetryCascade — Gün 1–10 Çalışma Panosu

Bu klasör ilk on günün uygulanabilir planıdır. Ana bilimsel sözleşmenin kaynağı TODO dosyaları değil; `configs/research_protocol.yaml`, `docs/system_architecture.md` ve bilimsel protokoldür.

## Durum

| Gün | Konu | Durum |
|---|---|---|
| 01 | Problem, literatür ve iskelet | Tamamlandı |
| 02 | Semantik ve araştırma protokolü | Tamamlandı; resolved schema Gün 3'e devredildi |
| 03 | Rust tipleri ve resolved config | Bekliyor |
| 04 | Deterministik scheduler ve asgari RNG | Bekliyor |
| 05 | Retry’siz pipeline | Bekliyor |
| 06 | Bounded queue ve Sink trigger | Bekliyor |
| 07 | Timeout, deadline ve lifecycle ledger | Bekliyor |
| 08 | Immediate ve exponential retry | Bekliyor |
| 09 | Full jitter ve retry budget | Bekliyor |
| 10 | Metrikler, recovery ve uçtan uca smoke | Bekliyor |

## Günlük dosyalar

- [Gün 1](gun_01.md)
- [Gün 2](gun_02.md)
- [Gün 3](gun_03.md)
- [Gün 4](gun_04.md)
- [Gün 5](gun_05.md)
- [Gün 6](gun_06.md)
- [Gün 7](gun_07.md)
- [Gün 8](gun_08.md)
- [Gün 9](gun_09.md)
- [Gün 10](gun_10.md)

## Sekiz ajanlı ortak karar

- Plan mevcut geniş haliyle başlangıç seviyesi için `NO-GO`, Must/Stretch ayrımıyla `CONDITIONAL GO`.
- Python tek config otoritesidir: protokolü doğrular, tek-run config’i tamamen çözer ve hash üretir.
- Rust yalnız tam çözülmüş JSON config’i strict biçimde okur, invariant’ları doğrular, simülasyonu yürütür ve hash/sürümleri çıktıya taşır.
- İlk 10 günde Parquet, PostgreSQL, 3000-run deney ve source-load-shedding implementasyonu zorunlu değildir.
- İlk 10 günün hedefi bilimsel sonuç değil, deterministik ve ledger’ları kapanan simülasyon çekirdeğidir.
- Test sayısı hedef değildir. Yaklaşık 20–30 odaklı unit test ve 3–5 elle doğrulanmış golden senaryo yeterlidir.

## Her günün ortak çalışma kuralı

- Önce kavramı 15–30 dakika öğren.
- Must maddelerini küçük parçalarla kendin yaz.
- AI kodu ancak ne yaptığını açıklayabiliyorsan kabul et.
- En az bir normal durum ve bir hata/sınır testi çalıştır.
- Gün sonunda ilgili kavramı koda bakmadan 2–5 dakika anlat.
- Stop gate geçmezse sonraki güne geçme; Stretch maddelerini kes.

## Global hard gate’ler

1. Gün 3'ün ilk işi olan full resolved-config schema ve golden fixture bitmeden Rust simülasyon modeline başlanmaz.
2. Scheduler deterministik değilse pipeline başlamaz.
3. Trigger work-conservation ve lifecycle ledger testleri geçmezse retry politikaları başlamaz.
4. Gün 10’da `resolved config → deterministik run → kapanan ledger → dört policy → recovery summary` zinciri geçmezse Parquet/PostgreSQL başlamaz.

## Kapsam koruma kuralı

Gün 10 iki günden fazla sarkarsa bilimsel çekirdekten kesinti yapılmaz. Önce source shedding, ayrıntılı production trace, ikincil metrikler ve PostgreSQL gösterimi ertelenir.
