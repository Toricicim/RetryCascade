# RetryCascade V1 — Dondurulmuş Sistem Mimarisi

Bu belge Gün 2 sistem modelini tanımlar. Sonuçlar görüldükten sonra bu semantikler sessizce değiştirilemez; değişiklik yeni `model_version` ve `protocol_hash` gerektirir.

## 1. Araştırılan mekanizma

V1 şu tek nedensel zinciri izole eder:

```text
geçici Sink kapasite düşüşü
→ kuyruk ve gecikme artışı
→ client-side timeout
→ yeni retry attempt'leri
→ toplam fiziksel işin artması
→ trigger kalktıktan sonra gecikmiş recovery veya operational non-recovery
```

Model bir PostgreSQL performans tahmincisi veya OSDI cache deneyinin birebir kopyası değildir.

## 2. Topoloji ve kapasite

```text
Poisson external logical arrivals
        ↓
Source/Ingress → Transform/Application → Sink/Database
```

- Dış arrival generator, Source aşamasının dışındadır.
- Üç aşama aynı generic `Stage` yapısını kullanır.
- Her aşamada ayrı worker havuzu ve yalnız bekleyen işleri sayan bounded FIFO queue bulunur.
- Nominal işleme hızları `140/120/100 work-unit/s`; bottleneck Sink'tir.
- `rho = lambda_external / 100` olarak tanımlanır; üç aşamanın ortalaması değildir.
- Source ve Transform headroom taşır; trigger yalnız Sink'i etkiler.

Cache V1 topolojisinde yoktur. Makaledeki `300 DB RPS / %90 hit / 3000 client RPS` aritmetiği yalnız vulnerability motivasyonudur.

## 3. Request ve attempt sözleşmesi

- `logical_request_id`, kullanıcının tek yararlı işini temsil eder ve goodput en fazla bir kez sayılır.
- İlk fiziksel deneme ve retry'lar ayrı `attempt_id` taşır.
- Attempt Source'a kabul edildiğinde per-attempt end-to-end timeout başlar; bütün queue beklemeleri süreye dahildir.
- Timeout client-side bir olaydır. Eski attempt sunucudan iptal edilmez ve kapasite tüketmeye devam eder.
- Retry, Source'tan yeni bir fiziksel attempt olarak başlar. `max_attempts` ilk denemeyi de içerir.
- Timeout yaşamış attempt'in sonradan bitmesi `late_completion` ve wasted work'tür; read-like RPC varsayımı altında client goodput sayılmaz.
- Goodput, kendi timeout'undan önce yanıt veren ilk attempt ile logical request başına en fazla bir kez oluşur.
- Logical başarıdan sonra henüz oluşturulmamış retry event'leri no-op olur. Queue'da veya service'te bulunan kopyalar V1'de iptal edilmez; tamamlanırsa duplicate/wasted work sayılır.
- V1 yalnız read-like/idempotent iş yükünü temsil eder. Write commit, side effect ve duplicate-write doğruluğu kapsam dışıdır.

## 4. Queue ve failure sözleşmesi

- `queue_capacity` yalnız bekleyen slotları ifade eder; çalışan işler worker state'inde tutulur.
- Downstream queue doluysa attempt anında `queue_rejected(stage)` olur.
- V1'de upstream worker blocking veya backpressure yoktur.
- Timeout ve queue rejection retryable cause olabilir; aynı cause için en fazla bir retry schedule edilir.
- Her logical request ve attempt tam olarak bir terminal ledger sınıfında kapanmalıdır; horizon sonunda çalışanlar ayrıca `outstanding/inflight_at_horizon` olur.

## 5. Trigger sözleşmesi

- Tek trigger, `[trigger_start, trigger_end)` aralığında Sink'in etkin service rate'ini `0.50 × nominal` yapar.
- Aktif işin kalan service demand'i kaybolmaz veya yeniden örneklenmez; yeni hız altında devam eder.
- Bunun için completion event'leri `service_epoch` taşır; eski epoch'a ait stale completion state'i değiştiremez.
- Trigger bittiğinde Sink nominal hıza tamamen döner ve dispatch yeniden yapılır.
- Queue, retry ve inflight birikimi temizlenmez; post-trigger sustaining effect bu state üzerinden gözlenir.

## 6. Aynı timestamp olay sırası

Scheduler anahtarı `(time_tick, priority, sequence_id)` şeklindedir. Zaman `u64` microsecond tick'tir ve taşmalı aritmetik hata üretir.

```text
1. Stage completion
2. Attempt timeout
3. Trigger boundary
4. Retry ready
5. Logical arrival
6. Queue sample
```

- Completion ve timeout aynı tick'teyse completion kazanır. Trigger sınırındaki tamamlanmış iş önce kapanır; aynı tick'teki yeni arrival ise trigger transition sonrasındaki kapasiteyi görür.
- Tek trigger kullanıldığı için aynı tick'te TriggerStart/TriggerEnd çakışması config validation tarafından reddedilir.
- Event cap'e ulaşmak `simulator_error`dır; `non_recovery` değildir.

## 7. Policy ve mitigation ayrımı

Ana dört policy:

1. `immediate`
2. `exponential`
3. `full_jitter`
4. `retry_budget`

İlk üç policy aynı max-attempt, timeout, overall deadline, retryable cause ve exponential base/cap değerlerini kullanır. `full_jitter` ile `exponential` arasındaki tek fark gecikmenin randomize edilmesidir. `retry_budget`, full-jitter zamanlamasına ek olarak retry token iznini sınırlar; ilk logical arrival'ları shed etmez.

**Source load shedding ayrı bir mitigation deneyidir; retry-budget kolunun içine gizlenmez.** Böylece recovery'nin retry sınırından mı, kullanıcı işini reddetmekten mi kaynaklandığı ayrılabilir. Source shedding çalıştırıldığında shed oranı mutlaka raporlanır.

## 8. Recovery ve maliyet

- Ana horizon `H=600 s`; sonuç `operational_non_recovery_within_horizon` olarak adlandırılır, kalıcı arıza iddia edilmez.
- Eşlenmiş no-trigger koşusu stabil değilse trigger koşusu metastability kanıtı olarak kullanılmaz.
- Recovery, üç ardışık 30 saniyelik pencerenin tamamında goodput ve queue eşiklerinin sağlanmasıyla belirlenir.
- `H=300/600/900 s` yalnız önceden belirlenmiş sensitivity analizidir.
- Goodput/p95 tek başına raporlanmaz; success, source shedding, abandonment, attempts exhausted ve amplification birlikte verilir.

## 9. Determinizm ve veri üretimi

- Arrival, service demand ve jitter için ayrı/keyed RNG stream'leri kullanılır.
- Policy değişince external arrival ve ortak service-demand trace hash'i değişmez.
- Her koşu `model_version`, `protocol_hash`, `rng_version`, seed ve semantic checksum kaydeder.
- Her event'i varsayılan olarak kalıcılaştırmak yerine aggregate ledger zorunlu, ayrıntılı event trace yalnız debug/sampled modda tutulur. Bu, binlerce koşuda disk patlamasını önler.

## 10. V1 kapsam dışı

- Cache state, hit-rate dynamics ve refill
- Garbage collection
- Lock contention ve leader-election churn
- Autoscaling ve load rebalancing
- Backpressure ve gerçek cancellation
- Non-idempotent writes ve side effects
- Çok servisli DAG ve birden çok retry katmanı

Bu mekanizmalar gelecekte aynı deney altyapısına ayrı modeller olarak eklenebilir; V1 sonuçlarının parçası değildir.
