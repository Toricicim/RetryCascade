# Gün 1–10 Veri Mühendisliği ve Artifact Sınırı

Bu belge ilk 10 günlük Rust çekirdeği planının veri mühendisliği açısından kapsamını sınırlar. Amaç, simülasyon semantiğini geliştirirken ileride değişmesi pahalı olacak veri sözleşmelerini erken görünür kılmak; Parquet ve PostgreSQL'i çekirdek doğru çalışmadan erkenden projeye sokmamaktır.

## Temel karar: Python–Rust sınırı

İlk turda tek config otoritesi Python'dır:

```text
research_protocol.yaml
        |
        v
Python: validate + matrix expansion + canonicalize + SHA-256
        |
        v
resolved_run_config.json
        |
        v
Rust: parse + type/range/overflow/invariant checks + simulate
```

- Python araştırma protokolünü genişletir ve her koşu için tamamen çözülmüş, tek-run JSON config üretir.
- Python `protocol_sha256` ve `run_config_sha256` değerlerini hesaplar. Hash hesabında hash alanlarının kendisi dışarıda bırakılır.
- Rust YAML protokolünü, factor listesini veya varsayılan değerleri yorumlamaz; yalnız resolved JSON okur.
- Rust canonical JSON ya da ikinci bir hash algoritması uygulamaz. Aksi hâlde iki dilde aynı normalizasyonu sürdürme yükü ve sessiz hash ayrışması doğar.
- Rust config içindeki hash ve sürüm alanlarını sonuç metadata'sına aynen taşır. Rust'ın sorumluluğu yapısal parse sonrasında simülasyon için gerekli semantic invariant'ları tekrar kontrol etmektir.
- `run_config_sha256`, tek bir bilimsel koşunun kimliğidir; `run_id` değildir. Aynı config tekrar çalıştırılabilir. İleride registry'deki yürütme kimliği ayrı `run_id` olur.

Bu nedenle ana yol haritasındaki Gün 3 maddesi `Canonical config hash` değil, `Python tarafından üretilen hash/sürüm alanlarını parse et ve çıktıya taşı` olarak uygulanmalıdır.

## Resolved run config'in asgari içeriği

Rust'ın simülasyon sırasında başka bir dosyaya veya Python varsayımına ihtiyaç duymaması gerekir. Resolved JSON en az şu grupları içermelidir:

- Şema ve provenance: `schema_version`, `protocol_version`, `model_version`, `rng_version`, `protocol_sha256`, `run_config_sha256`.
- Koşu faktörleri: `seed`, `rho_basis_points`, `policy`, trigger var/yok bilgisi ve trigger süresi.
- Tam çözülmüş model sabitleri: üç stage'in worker, queue ve nominal rate değerleri; timeout, overall deadline, max attempts; policy'nin sayısal parametreleri.
- Zaman sınırları: arrival başlangıcı, warm-up/baseline/trigger başlangıcı, post-trigger artifact horizon ve safety event cap.
- Ölçüm sözleşmesi: queue sample interval, recovery window sayısı/süresi ve eşikler.
- Deney uygunluğu: `analysis_eligible` ve `derived_from_protocol`.

Resolved config küçük tutulmalı ama “ince referans” olmamalıdır. Yalnız `policy=full_jitter` yazıp base/cap değerlerini Rust içinde varsaymak reproducibility'yi bozar. Buna karşılık 30 seed veya bütün rho matrisi tek-run config'e kopyalanmamalıdır.

## İlk 10 günde tutulacak çıktılar

İlk 10 gün için üç küçük ve sürümlü JSON sözleşmesi yeterlidir:

1. `resolved_run_config.json`: Python'ın Rust'a verdiği değişmez girdi.
2. `run_summary.json`: Rust'ın koşu sonundaki sayaçları, metrikleri, bitiş durumunu ve provenance alanlarını taşıyan küçük sonuç.
3. `manifest.json`: Artifact listesi, byte/row count, SHA-256, schema version ve run completion durumu. Gün 10'da şema/fixture olarak tasarlanır; güvenli dosya yazımı Gün 11'de uygulanır.

Geliştirme fixture'larında ayrıntılı olaylar küçük, insan tarafından okunabilir `events.ndjson` veya bellekte golden liste olarak tutulabilir. İlk 10 günde genel amaçlı event arşivi, full production trace veya 3000-run çıktısı üretilmemelidir.

`run_summary.json` için asgari alanlar:

- Kimlik/provenance: config hash'leri, version'lar, seed, policy ve factor değerleri.
- Durum: `COMPLETED`, `EVENT_CAP_REACHED` veya `SIMULATOR_ERROR`; yalnız `COMPLETED` analiz için adaydır.
- Sayaçlar: logical arrivals/outcomes, attempts/outcomes, retries, late/duplicate completion ve event count.
- Özet metrikler: goodput, retry amplification, queue özetleri ve recovery sonucu.
- Determinizm kanıtı: event semantic checksum ve common workload trace checksum.

Float JSON üretiminde dil farkı yaşamamak için kimlik ve config alanlarında integer domain korunmalıdır. Analitik oranlar summary'de float olabilir; bunlar config hash'ine dahil edilmez ve bilimsel yeniden üretimin asıl kanıtı ham sayaçlardır.

## Gün gün artifact olgunlaşması

### Gün 1–2

- Protocol ve run-config şemaları sürümlenir.
- Python canonicalization ve hash için tek otorite olur.
- Bir resolved config örneği golden fixture olarak eklenir.
- Şimdilik Parquet tablosu veya PostgreSQL migration yazılmaz.

### Gün 3

- Rust yalnız resolved JSON config'i typed struct'lara parse eder.
- Eksik alan, bilinmeyen alan, geçersiz enum, pozitiflik, zaman aritmetiği ve `u64` overflow reddedilir.
- Rust hash'i yeniden üretmez; input hash/version alanlarını echo eden bir parse testi yeterlidir.

### Gün 4–7

- Scheduler, RNG, pipeline ve lifecycle testleri küçük golden event fixture'ları kullanır.
- Event record alan adları taslak hâle gelir; her iç Rust event'inin kalıcı artifact olması gerekmez.
- Her test run'ında semantic event checksum ve workload checksum üretimi başlanır.
- Dosyaya yüksek hacimli event yazımı eklenmez; bu günlerin başarısı veri hacmi değil invariant doğruluğudur.

### Gün 8–9

- Policy sayaçları ortak summary sözleşmesine bağlanır.
- Bütün politikalar aynı resolved config ve aynı sayaç adlarını kullanır.
- Policy'ye özel debug alanları ana analiz şemasına sızdırılmaz.

### Gün 10

- `run_summary` typed Rust struct ve versioned JSON fixture olarak tamamlanır.
- Recovery ve metric sonuçları mümkün olduğunca ham pay/payda sayaçlarıyla birlikte yazılır.
- Gelecek Parquet tabloları için event ve queue-series alan listesi dondurulur; henüz Parquet conversion yapılmaz.
- Manifest şeması ve örneği yazılır; atomic temp-file/rename uygulaması Gün 11'e bırakılır.

## Parquet ne zaman başlamalı?

Parquet implementasyonu Gün 12'de başlamalıdır. Gün 10'da yalnız şema taslağı hazırlanması entegrasyon riskini azaltır. Daha erken başlamak şu riskleri getirir:

- Event semantiği değişirken sürekli migration/column churn.
- Rust model hataları ile serialization hatalarının birbirine karışması.
- Yeni başlayan geliştiricinin çekirdek invariant'lardan uzaklaşması.

Python, Rust'ın tamamlanmış raw event/queue çıktısını doğruladıktan sonra Parquet'e çevirmelidir. Rust'ın ilk turda Arrow/Parquet dependency almasına gerek yoktur. Büyük event satırları PostgreSQL'e hiçbir aşamada yazılmaz.

## PostgreSQL ne zaman başlamalı?

Migration ve registry kodu Gün 13'te başlamalıdır. Buna rağmen Gün 10'a kadar PostgreSQL'in ihtiyaç duyacağı metadata alanları `manifest.json` içinde belirlenmiş olmalıdır. Böylece veritabanı simülatörün iç modeli hâline gelmez; yalnız kontrol düzlemi olur.

PostgreSQL'de tutulacaklar düşük hacimlidir:

- experiment ve execution/run kimliği,
- config/protocol hash ve sürümler,
- seed/policy/factor değerleri,
- yaşam döngüsü durumu,
- artifact URI, schema version, checksum, row count,
- validation sonucu ve seçilmiş summary metrikleri.

PostgreSQL yokken Rust simülasyonu ve tek-run reproduce akışı çalışabilmelidir. Bu ayrım yerel geliştirmeyi ve hata ayıklamayı kolaylaştırır.

## Kabul kapıları

İlk 10 günün veri/artifact açısından tamamlanmış sayılması için:

- Aynı protocol ve factor seçimi aynı resolved config byte/hash sonucunu verir.
- Rust config'e varsayılan bilimsel parametre eklemez.
- Rust bilinmeyen config alanını reddeder.
- `run_summary` sayaçları lifecycle ledger ile kapanır.
- Tamamlanmamış koşu `COMPLETED` veya analysis-eligible görünmez.
- Aynı resolved config, seed ve binary aynı semantic checksum'ı verir.
- Policy değişince common workload checksum'ı değişmez.
- Parquet/PostgreSQL olmadan bütün Gün 1–10 unit ve golden testleri çalışır.

## Kapsam uyarısı

20 günlük ve yeni başlayan odaklı planda Gün 1–10'a Parquet, PostgreSQL, crash-safe orchestrator ve tam artifact writer'ı birlikte çekmek gerçekçi değildir. İlk yarının teslimi “veri platformu” değil, açık giriş/çıkış sözleşmesi bulunan doğrulanmış simülasyon çekirdeğidir. Veri hattı bu çekirdeğin sabit sözleşmesi üzerine Gün 11–14'te kurulmalıdır.
