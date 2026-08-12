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
- Nominal işleme hızları `140/120/100 work-unii/s`; bottleneck Sink'tir.
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

### No-trigger kontrol koşusu

No-trigger kontrol koşusunda `trigger.enabled = false` olur. Sink kapasitesi koşu boyunca nominal değerinde kalır. Trigger başlangıç ve bitiş olayları oluşturulmaz.

Aynı rho, policy ve seed değerlerine sahip trigger koşusuyla aynı external arrival ve service-demand örnekleri kullanılır. Böylece iki koşu arasındaki temel fark yalnızca geçici kapasite kaybıdır.

Önemli fikir: iki deneyde aynı trafik var; yalnız birinde arıza var.

### Post-trigger gözlem süresi

Recovery horizon, trigger bittikten sonra başlar.

trigger_end = trigger_start + trigger_duration
run_end = trigger_end + post_trigger_observation_duration

Örneğin trigger 300. saniyede başlayıp 10 saniye sürerse `trigger_end` 310. saniyedir. Post-trigger observation 600 saniyeyse değerlendirme 910. saniyeye kadar devam eder.

Bu ayrım önemlidir; çünkü 30 saniyelik ve 1 saniyelik trigger'ların ikisine de trigger bittikten sonra eşit toparlanma süresi vermiş oluyoruz.

### Kalan işin yeniden zamanlanması

Trigger başladığında veya bittiğinde service içindeki iş kaybolmaz ve yeniden örneklenmez. Önce eski hız altında tamamlanan work hesaplanır. Kalan work, yeni service rate kullanılarak yeni bir completion zamanına dönüştürülür.

Kesirli tick oluşursa completion süresi yukarı yuvarlanır (`ceiling`). Böylece iş gereken miktardan daha erken tamamlanmaz.

Her yeniden zamanlamada attempt'in `service_epoch` değeri artırılır. Scheduler'da kalan eski completion event'i geldiğinde epoch eşleşmiyorsa state'i değiştirmeden yok sayılır ve `stale_completion_events` sayacı artırılır.

## 6. Aynı timestamp olay sırası

Scheduler anahtarı `(time_tick, priority, sequence_id)` şeklindedir. Zaman `u64` microsecond tick'tir ve taşmalı aritmetik hata üretir.

```text
1. Stage completion
2. Attempt timeout
3. Logical deadline
4. Trigger boundary
5. Retry ready
6. Logical arrival
7. Queue observation
```

### Source kuyruğu dolu olduğunda

Bir attempt Source'a geldiğinde bütün worker'lar meşgulse ve waiting queue da doluysa attempt sisteme alınmaz.

Bu attempt:

- `queue_rejected(source)` olarak kaydedilir.
- Source'a kabul edilmediği için attempt timeout'u başlatılmaz.
- Fiziksel iş yapmadığı için servis kapasitesi tüketmez.
- Queue rejection retryable bir sonuçtur.
- Logical request'in deadline ve attempt hakkı uygunsa retry politikası daha sonra yeni bir attempt planlayabilir.

Örnek: Source'ta 1 worker ve 2 bekleme yeri vardır. Worker çalışırken iki attempt kuyrukta bekliyorsa gelen dördüncü attempt reddedilir.

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

### Retry budget kapsamı ve aynı-tick sırası

Retry budget her logical request için ayrı değil, bir simülasyon koşusundaki bütün retry'lar tarafından paylaşılan run-global token bucket'tır. İlk attempt token tüketmez; yalnız retry oluşturulurken token gerekir. Token yoksa retry oluşturulmaz ve `retry_denied` sayacı artırılır.

Yalnızca bir logical request için ilk unique ve zamanında başarı token refill'i üretir. Late veya duplicate completion refill üretmez. Token sayısı hiçbir zaman sıfırın altına veya tanımlı kapasitenin üstüne çıkamaz.

Bir completion ile `RetryReady` aynı tick'e denk gelirse event önceliği nedeniyle completion önce işlenir. Completion refill hakkı kazanıyorsa token önce eklenir; ardından `RetryReady` güncel bucket durumunu görür. Böylece sonuç, event'in scheduler'a eklenme sırasına bağlı olmaz.

**Source load shedding ayrı bir mitigation deneyidir; retry-budget kolunun içine gizlenmez.** Böylece recovery'nin retry sınırından mı, kullanıcı işini reddetmekten mi kaynaklandığı ayrılabilir. Source shedding çalıştırıldığında shed oranı mutlaka raporlanır.

## 7.1 Analitik queue doğrulama fixture'ı

Ana deneyde service demand lognormal dağılımdan gelir; bu nedenle ana model `M/M/1/K` değildir. `M/M/1/K` analitik sonuçları ana lognormal koşuları doğrulamak için kullanılmayacaktır.

Simulator için ayrı bir verification fixture'ında arrival süreleri exponential, service süreleri exponential, tek server ve bounded queue kullanılacaktır. Yalnız bu özel fixture `M/M/1/K` analitik stationary ölçüleriyle karşılaştırılır. Bu kontrol, gerçek sistem geçerliliğini değil simulator'ın temel queue mekanizmasını doğru uyguladığını sınar.

## 8. Recovery ve maliyet

- Ana horizon `H=600 s`; sonuç `operational_non_recovery_within_horizon` olarak adlandırılır, kalıcı arıza iddia edilmez.
- Eşlenmiş no-trigger koşusu stabil değilse trigger koşusu metastability kanıtı olarak kullanılmaz.
- Recovery, üç ardışık 30 saniyelik pencerenin tamamında goodput ve queue eşiklerinin sağlanmasıyla belirlenir.
- `H=300/600/900 s` yalnız önceden belirlenmiş sensitivity analizidir.
- Goodput/p95 tek başına raporlanmaz; success, source shedding, abandonment, attempts exhausted ve amplification birlikte verilir.

Trigger bittiği anda 30 saniyelik parçalar hâlinde sistemi ölçeceğiz:

Trigger 310. saniyede bitti.

1. pencere: `[310, 340)`
2. pencere: `[340, 370)`
3. pencere: `[370, 400)`

### Recovery pencerelerinin hizalanması

Recovery pencereleri `trigger_end` anında başlar ve birbiriyle çakışmaz.

Bir completion tam pencerenin bitiş anında gerçekleşirse biten pencereye değil, sonraki pencereye dahil edilir. Pencereler `[start, end)` biçimindedir.

Recovery için arka arkaya üç pencerenin de tanımlanan goodput ve queue koşullarını sağlaması gerekir.

### Queue recovery metriği

Her 30 saniyelik recovery penceresinde Source, Transform ve Sink waiting queue uzunlukları toplanır. Worker içinde çalışan işler bu queue değerine dahil edilmez.

Ortalama, belirli aralıklarla alınan sample'ların basit ortalamasıyla değil, queue değerinin her seviyede ne kadar süre kaldığı kullanılarak hesaplanır:

```text
time_weighted_mean_queue = sum(queue_value × duration) / window_duration
```

Örneğin queue 10 saniye boyunca 0, sonraki 10 saniye boyunca 9 ve son 10 saniye boyunca tekrar 0 ise:

```text
(0 × 10 + 9 × 10 + 0 × 10) / 30 = 3
```

`queue_sample_interval_us = 100000` ile alınan 100 ms sample'lar yalnızca debug ve görselleştirme içindir. Bu interval değiştirildiğinde bilimsel recovery sonucu değişmemelidir.

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
