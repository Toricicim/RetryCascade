# RetryCascade Araştırma Sorusu

## Metastable failure

Metastable failure, sistemi kötü duruma sokan geçici tetikleyici ortadan kalkmasına rağmen bir geri besleme mekanizması nedeniyle düşük performans durumunun devam etmesidir. Sistem tamamen bozulmuş değildir, ancak normal çalışmasıyla makul sürede toparlanamaz. Yükün ciddi biçimde azaltılması veya retry politikasının değiştirilmesi gibi güçlü bir müdahale gerekebilir.

Bu projede metastable failure, matematiksel olarak sonsuza kadar süren bir durum anlamına gelmeyecektir. Trigger sona erdikten sonra sistemin 600 saniyelik gözlem süresi içinde toparlanamaması, `operational non-recovery` olarak tanımlanacaktır.

## Stable, vulnerable ve metastable ayrımı

- **Stable:** Sistem normal yükü işler ve tanımlanan geçici trigger sonrasında dış müdahale olmadan toparlanabilir.
- **Vulnerable:** Sistem normal koşullarda sağlıklı görünür; fakat kapasite payı azaldığı için yeterli şiddetteki bir trigger sistemi metastable duruma taşıyabilir.
- **Metastable:** Trigger sona ermesine rağmen sustaining effect nedeniyle düşük goodput ve yüksek kuyruk durumu devam eder.

Stable veya vulnerable olmak sistemin değişmez bir özelliği değildir. Bu durumlar başlangıç yüküne, trigger süresine, timeout değerine, retry politikasına, kuyruk sınırlarına ve sistem kapasitesine bağlıdır.

## Trigger ve sustaining effect farkı

- **Trigger:** Sistemi normal çalışma durumundan kötü duruma iten geçici olaydır. Kısa süreli ağ kesintisi veya veritabanı yavaşlaması buna örnek olabilir.
- **Sustaining effect:** Trigger bittikten sonra kötü durumu devam ettiren geri besleme mekanizmasıdır. Retry nedeniyle oluşan ek fiziksel iş yükü bu etkiye örnektir.

Trigger arızayı başlatır; fakat metastable failure'ın uzun sürmesinin temel nedeni sustaining effect'tir.

## Mitigation

Mitigation, başlamış geri besleme döngüsünü kırmak ve sistemi normal çalışma bölgesine döndürmek için uygulanan müdahaledir. Load shedding, retry bütçesi, throttling, kapasite artırma, restart ve sorunlu yapılandırmayı geri alma buna örnektir. RetryCascade'de retry budget ana politika kollarından biridir; source load shedding ise ayrı mitigation deneyi olarak tutulur. Böylece sistemi retry'ı sınırlamanın mı, kullanıcı işini reddetmenin mi toparladığı karıştırılmaz. Toparlanmanın bedeli gizlenmeyecek; reddedilen veya tamamlanamayan logical request'ler ayrı metrik olarak raporlanacaktır.

## Retry ile work amplification

Bir logical request için gönderilen ilk deneme zamanında cevap vermediğinde yeni attempt'ler oluşturulabilir. İlk attempt sunucu tarafında hâlâ çalışıyorsa retry, aynı kullanıcı amacı için ek fiziksel çalışma üretir.

Örneğin bir kullanıcı yalnızca bir kez işlem başlatmasına rağmen ilk attempt ve iki retry sonucunda sistem üç ayrı attempt işleyebilir:

```text
1 logical request
→ 1 ilk attempt
→ 2 retry
= 3 fiziksel attempt
```

Bir logical request başına yapılan fiziksel iş miktarının bu şekilde artmasına `work amplification` denir. Ek attempt'ler kapasiteyi aşarsa kuyruk büyüyebilir; büyüyen kuyruk daha fazla timeout ve retry üreterek geri besleme döngüsü oluşturabilir.

## Araştırma sorusu

Full-jitter zamanlaması aynı tutulduğunda run-global retry budget'ın operasyonel toparlanmaya ek etkisi, başlangıç yükü ve geçici Sink kapasite kaybının süresiyle nasıl değişir? Bu etki; zamanında tamamlanan benzersiz logical request oranı, deadline abandonment ve fiziksel work amplification maliyetleriyle birlikte nasıl değerlendirilmelidir?

Immediate retry, deterministic exponential backoff, full jitter ve retry budget kollarının tamamı uygulanmış politika paketleri olarak ayrıca karşılaştırılacaktır. Ancak bu paket karşılaştırması, tek başına jitter veya budget mekanizmasının nedensel etkisi diye yorumlanmayacaktır.

## Hipotezler

### Birincil H1 — Aynı jitter altında retry budget etkisi

Önceden tanımlanan yüksek-stres hücrelerinde (`rho ∈ {0.80, 0.90, 0.95}` ve trigger süresi `10 s` veya `30 s`), aynı full-jitter zamanlaması altında retry budget açıkken 600 saniye içinde operasyonel toparlanma olasılığı, budget kapalı full-jitter kolundan daha yüksek olacaktır.

Bu hipotezin sıfır ve alternatif biçimi şöyledir:

- **H0:** `retry_budget − full_jitter` toparlanma olasılığı farkı sıfırdan büyük değildir.
- **H1:** `retry_budget − full_jitter` toparlanma olasılığı farkı sıfırdan büyüktür.

Bu karşılaştırmada iki kolun jitter algoritması ve backoff parametreleri aynıdır; değişen mekanizma token budget'tır. Sonuç her `rho × trigger süresi` hücresinde ayrıca gösterilecektir. Yüksek-stres hücreleri için eşit ağırlıklı özet verilebilir, ancak bu özet ters yönlü hücresel etkileri gizlemek için kullanılmayacaktır.

Toparlanma kazanımı tek başına "daha iyi politika" anlamına gelmez. Aynı hücrelerde zamanında benzersiz başarı oranı, deadline abandonment ve attempts-per-logical-request birlikte raporlanacaktır. Budget daha çok recovery sağlarken kullanıcı başarısını düşürürse sonuç üstünlük değil, trade-off olarak adlandırılacaktır.

### İkincil araştırma soruları

- Politika etkisi başlangıç yükü ve trigger süresine göre yön veya büyüklük değiştiriyor mu? Koşula bağlı sıralama ve crossover sonuçları, belirsizlikleriyle birlikte betimleyici olarak raporlanacaktır; crossover varmış gibi önceden varsayılmayacaktır.
- Yalnız kuyruk durumuna bakan recovery tanımı ile goodput ve kuyruğu birlikte kullanan ana recovery tanımı hangi koşullarda farklı sınıflandırma yapıyor? Bu bir ölçüm duyarlılığı analizidir; gerçek sistem için dış ground truth olmadan "queue-only yanlıştır" denmeyecektir.
- Süreye özel `rho_50` değerleri ikincil boundary özetidir. Gözlenen gridin dışında extrapolation yapılmayacak ve crossing yoksa sonuç censored/not estimable olarak raporlanacaktır.
- Deterministic exponential ile standart full-jitter karşılaştırması yalnız uygulanan iki politika paketinin karşılaştırmasıdır. Mean-matched kontrol eklenmeden saf jitter/senkronizasyon nedenselliği iddia edilmeyecektir.

Bu belgedeki birincil H1, `configs/research_protocol.yaml` içindeki `analysis.h1` alanının insan tarafından okunabilir karşılığıdır. Birincil hipotez için YAML protokolü sürümlü ve makine-okunur otoritedir; metinler onunla çelişemez. Pilot tohumları birincil analize alınmayacak; ana deney başlamadan önce final grid, seed sayısı, belirsizlik yöntemi ve censoring kuralları ayrıca dondurulacaktır.

## Projenin iddia etmeyeceği şeyler

- Bir retry politikasının bütün gerçek sistemlerde diğerlerinden daha iyi olduğu iddia edilmeyecektir.
- Simülasyonun gerçek bir üretim sisteminin ne zaman çökeceğini tahmin ettiği söylenmeyecektir.
- Modelin PostgreSQL veya başka bir veritabanı motorunun gerçek performansını temsil ettiği iddia edilmeyecektir.
- Literatürde bilinmeyen yeni bir failure türünün keşfedildiği söylenmeyecektir.
- Sonuçlar, deneyde incelenmeyen yük, trigger ve sistem koşullarına genellenmeyecektir.
- Simülasyon sonuçları tek başına güçlü sistemler konferansı düzeyinde dış geçerlilik veya üretim sistemi rehberi olarak sunulmayacaktır.

## Temel kaynak

Bronson, N., Aghayev, A., Charapko, A. ve Zhu, T. (2021). *Metastable Failures in Distributed Systems*. HotOS '21. https://doi.org/10.1145/3458336.3465286
