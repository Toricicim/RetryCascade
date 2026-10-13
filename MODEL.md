# RetryCascade model sözleşmesi

Bu belge 0.2.0 workbench'inin teknik referansıdır. Gerçek production tahmin
modeli veya gelecekte yapılacağı garanti edilen bir roadmap değildir.

## Logical request ve physical attempt

Logical request kullanıcının tek amacıdır; physical attempt ilk deneme veya
bir retry'dır. İlk kabul edilen, deadline içinde tamamlanmış cevap goodput
sayılır. Late/duplicate tamamlamalar iş tüketebilir ama ikinci bir kullanıcı
başarısı üretmez. FIFO waiting capacity servis içindeki işlerden ayrıdır.
Timeout sunucu işini otomatik durdurmaz.

V1 strict-RPC sözleşmesinde timed-out attempt sonradan başarı kazanamaz.
V2'nin `accept_timed_out_completion` sensitivity'si logical deadline'a kadar
ilk cevabı kabul eden parent-level receiver'ı temsil eder. Retry limiti dolsa
bile canlı iş varsa beklenebilir; deadline yine son sınırdır.
`budget_denial_finishes_logical_request: false` yeni retry iznini keser, mevcut
işi bekler. Bu alternatifler cancellation, deduplication veya strict RPC'nin
bug fix'i değildir. Eski varsayılan davranış korunur; alternatifler config
hash'ine dahil edilen açık alanlardır.

## Zaman ve rastgelelik

Integer microsecond tick, checked arithmetic ve insertion sequence kullanılır.
Aynı tick'te completion/pre-admission failure notification → attempt timeout
→ logical deadline → trigger boundary → retry ready → arrival → queue
observation sırası vardır. Completion aynı tick'te deadline'dan önce gelir;
RetryReady deadline ile eşitse deadline kazanır.

Sink trigger `[start,end)` aralığında etkindir; kalan service demand korunur,
hız değişiminde completion yeniden zamanlanır, eski epoch event stale sayılır.
Service duration integer ceiling ile hesaplanır.

Keyed ChaCha20; arrival, service, failure ve jitter namespace'lerini ayırır.
V2 üç potential attempt'lik ortak bankayı no-retry dahil denetler; bankadaki
her attempt gerçekleştirilmez. Full jitter ve budget aynı jitter çekilişini
kullanır. Request-stable demand aynı isteğin bütün retry'larında aynı intrinsic
işi taşır. Alternatif bağımsız demand ayrı sensitivity'dir.

## Budget ve hata modeli

Budget run-global'dır; ilk attempt token gerektirmez. İzinli retry kota tüketir;
yalnız ilk unique on-time logical success refill yapar. Late ve duplicate
completion refill yapmaz. Completion/refill aynı tick RetryReady'den önce
işlenir. Budget denial ve attempts-exhausted ayrı terminal nedenlerdir.

V2 IID pre-admission failure queue/worker/service work tüketmez, 1ms sonra
retryable hata bildirir; service timeout başlamaz. Response loss, correlated
outage veya gerçekleşmiş DB işinin hata dönmesi modellenmez. Nominal rho,
gerçek admission veya tüketilen work değildir: hatalar bazı yükü Source'a
ulaşmadan azaltabilir.

Üç attempt altında jitter upper bound ilk retry'da 25ms, ikinci retry'da 50ms.
200ms cap'e ulaşılmaz; cap etkisi izole edilmiş değildir. Exponential/full-jitter
paket kıyası saf randomization etkisi değildir; bekleme ortalamaları da farklıdır.

## Recovery ile kullanıcı başarısının ayrımı

Baseline dört 30s pencere kullanır: G0 median completion-time goodput; Q0,
exact time-weighted mean waiting occupancy'nin nearest-rank p95'idir. V2
eligibility queue slope/goodput CV/minimum arrivals ile ayrıca değerlendirilir;
planlı hatalar tek başına baseline instability değildir.

Resmi recovery: trigger bitişine hizalı üç ardışık, non-overlapping 30s pencere
hem `G >= .95 * G0` hem `Q <= Q0 + .05 * K` sağlamalıdır. İlk karar en erken
90s gözlem ister; sonsuza kadar sağlıklı kalma garantisi değildir. 300/600/900s
horizon'ları, sıfır/olmayan baseline ve gözlenmemiş horizon ayrı tutulur.
Wilson aralıkları yalnız eligible bağımsız seed'lerin koşullu binomial özetidir.

Calendar trace completion-time goodput ve service-time work/busy'yi sayar.
Arrival cohort kullanıcıların geldiği aralığa göre son başarı/failure'ını
özetler. Aynı interval'de tüketilen iş aynı interval'de gelmiş kullanıcıların
işiyle özdeş değildir. Son deadline cohort'u tamamen censored bırakılır;
hızlı başarılar seçilerek survivor bias yaratılmaz.

Optional trace nominal 30s grid kullanır; reference incident end ve sonraki
60s sınırları ayrıca bölünür. Matched no-trigger aynı calendar interval ve
horizon'a sahiptir. Queue area/work/busy integralleri tamdır. Event sayaçları
`[start,end)`; engine'in run-end anındaki event'i trace dışında kalabilir.
Retrospective cohort'lar `mature_available_at_us` taşır; canlı feature değildir.

## Betimleyici 0–100 indeks

K waiting capacity, Q exact bin mean queue, G bin goodput rate:

```text
goodput_health = clip(G / (.95 * G0), 0, 1)
Qlimit        = Q0 + .05 * K
queue_health  = clip(1 - max(0, Q - Qlimit) / (K - Qlimit), 0, 1)
display_index = 100 * min(goodput_health, queue_health)
```

Qlimit >= K ise bounded queue bileşeni 1. Kullanılabilir stable baseline yoksa
normalize özellikler null; ham ölçümler ayrıca sunulur. Bu öğrenilmiş skor,
doğrulanmış bilimsel indeks veya policy ranking değildir. 100 yalnız bin
hedeflerinin geçmesidir; tüm kullanıcıların başarısı veya resmi sustained
recovery değildir. Rounded summary G0/Q0 kullanır; exact Rust gate'in yerine geçmez.

EWMA (60s half-life), adjacent-bin değişim hızı, headroom ve normalize
goodput-deficit/queue-excess alanı yalnız geçmiş tamamlanmış bin'lerden türetilir.
Trailing windows exact whole-bin coverage ister; geleceğe veya bin içi
interpolasyona bakılmaz. Partial bin'ler süreleriyle değerlendirilir.
Grafik gölgesi seed min–max, CI değildir.

## Analitik referans ve iddia sınırı

Kapasite/timeout/deadline sorunu olmayan IID p hata, üç attempt: no-retry
başarı `1-p`, yeterli budget ile `1-p^3`. Success-refill r için stationary
ideal üst referans `min(1-p^3, (1-p)/(1-r*(1-p)))` olur. Başlangıç token'ları,
finite bucket discard, transient/queue/deadline davranışı equality varsayımını
bozabilir. Limitlerle uyuşmak özgün keşif değil, açıklayıcı kontroldür.

Paired bootstrap birimi seed; aynı hücrelerde jitter ve no-retry referansları
vardır. Çoklu keşif karşılaştırmaları confirmation değildir. Küçük sayısal
farklardan veya yalnız survivor latency'den otomatik kazanan çıkarılmaz.
Work CPU/para ölçümü değildir; production recovery tahmini verilmez.

## İlgili birincil kaynaklar

- [Metastable Failures in Distributed Systems](https://sigops.org/s/conferences/hotos/2021/papers/hotos21-s11-bronson.pdf)
- [Metastable Failures in the Wild](https://www.usenix.org/system/files/osdi22-huang-lexiang.pdf)
- [Analyzing Metastable Failures](https://sigops.org/s/conferences/hotos/2025/papers/hotos25-106.pdf)
- [RetryGuard](https://arxiv.org/abs/2511.23278)

Kaynaklar modelin motivasyonudur; RetryCascade'in özgünlüğü veya gerçek sisteme
kalibrasyonu kanıtlanmış kabul edilmez.
