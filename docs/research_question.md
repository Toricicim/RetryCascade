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

Farklı başlangıç yükleri ve geçici yavaşlama süreleri altında immediate retry, exponential backoff, full jitter ve retry budget politikaları sistemin operasyonel toparlanma olasılığını, toparlanma süresini ve oluşturduğu ek iş yükünü nasıl etkiler? Source load shedding'in recovery ve reddedilen kullanıcı işi arasındaki maliyeti ayrı mitigation deneyinde nasıl değişir?

## Hipotezler

### H1 — Retry budget ve toparlanma sınırı

Trigger süreleri üzerinden ortalama alındığında retry budget politikasının yüzde 50 toparlanma sınırı `rho_50`, immediate retry politikasından en az `0.05` daha yüksek olacaktır.

### H2 — Retry senkronizasyonu

Full jitter, aynı koşullardaki deterministic exponential backoff politikasına kıyasla bir saniyelik pencerelerde peak retry amplification'ı en az yüzde 10 azaltacaktır.

Bu belgedeki H1 ve H2, `configs/research_protocol.yaml` içindeki `analysis.h1` ve `analysis.h2` alanlarının insan tarafından okunabilir karşılığıdır. Birincil hipotezler için YAML protokolü sürümlü ve makine-okunur otoritedir; metinler onunla çelişemez.

## Projenin iddia etmeyeceği şeyler

- Bir retry politikasının bütün gerçek sistemlerde diğerlerinden daha iyi olduğu iddia edilmeyecektir.
- Simülasyonun gerçek bir üretim sisteminin ne zaman çökeceğini tahmin ettiği söylenmeyecektir.
- Modelin PostgreSQL veya başka bir veritabanı motorunun gerçek performansını temsil ettiği iddia edilmeyecektir.
- Literatürde bilinmeyen yeni bir failure türünün keşfedildiği söylenmeyecektir.
- Sonuçlar, deneyde incelenmeyen yük, trigger ve sistem koşullarına genellenmeyecektir.

## Temel kaynak

Bronson, N., Aghayev, A., Charapko, A. ve Zhu, T. (2021). *Metastable Failures in Distributed Systems*. HotOS '21. https://doi.org/10.1145/3458336.3465286
