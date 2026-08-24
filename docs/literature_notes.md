# RetryCascade Literatür Notları

Bu dosya okunan kaynaklardan çıkarılan kişisel notları içerir. Bir bölüm yalnız ilgili kaynak okunduktan sonra doldurulmalıdır.

## 1. Metastable Failures in Distributed Systems — HotOS 2021

**Kaynak:** Bronson, N., Aghayev, A., Charapko, A. ve Zhu, T. (2021). *Metastable Failures in Distributed Systems*.  
**Bağlantı:** https://doi.org/10.1145/3458336.3465286

### Ana fikirler

- Metastable failure, geçici trigger ortadan kalktıktan sonra sustaining effect nedeniyle kötü sistem durumunun devam etmesidir.
- Stable sistem tanımlanan trigger sonrasında kendiliğinden toparlanabilir; vulnerable sistem normal görünmesine rağmen aynı trigger sonrasında metastable duruma geçebilir.
- Trigger problemi başlatır, sustaining effect ise problemi devam ettirir.
- Retry, bir logical request için birden fazla fiziksel attempt oluşturarak work amplification üretebilir.
- Cache kaybı, pahalı hata işleme yolu ve connection-pool politikaları da geri besleme döngüsü oluşturabilir.
- Makale gerçek sistemlerdeki mekanizmayı destekler; RetryCascade simülatörünün sayısal parametrelerini doğrulamaz.

### RetryCascade açısından anlamı

- Logical request ile attempt ayrı kimliklerle izlenmelidir.
- Trigger sona erdikten sonraki davranış ayrıca ölçülmelidir.
- Goodput, toplam tamamlanan fiziksel işten ayrı tutulmalıdır.
- Proje gerçek üretim sistemini tahmin eden bir model olarak sunulmamalıdır.

## 2. Metastable Failures in the Wild — OSDI 2022

**Kaynak:** Huang, L. ve diğerleri (2022). *Metastable Failures in the Wild*.  
**Bağlantı:** https://www.usenix.org/conference/osdi22/presentation/huang-lexiang

**Okuma durumu:** İlk okuma tamamlandı; deney bölümlerine uygulama sırasında yeniden dönülecek.

### Kapsam ve temel bulgular

- Çalışma toplam **22 metastable failure olayını 11 kurumdan** inceler. Table 1 kamuya açık kaynaklardan bulunan 21 olayı içerir; Twitter'daki garbage-collection vakası içeriden incelenen ek olaydır.
- Olay raporlarındaki trigger'ların yaklaşık `%45`i mühendislik hatalarıyla (hatalı config/kod dağıtımı veya latent bug), yaklaşık `%35`i load spike ile ilişkilendirilmiştir. Vakaların yaklaşık `%45`inde birden fazla trigger vardır.
- İki geniş trigger ailesi tanımlanır: **organic load'u artıran load spike** ve **organic capacity'yi düşüren olay**.
- İki geniş amplification ailesi tanımlanır: **workload amplification** ve **capacity-degradation amplification**.
- Retry, incelenen olayların yarısından fazlasında görülen en yaygın sustaining effect'tir.
- Bildirilen kesintiler yaklaşık `1.5–73.53 saat` aralığındadır; süresi bildirilen olaylarda `4–10 saat` aralığı en sık görülen gruptur.
- Vulnerability ikili bir özellik değildir. Metastable duruma geçiş; başlangıçtaki vulnerability derecesiyle trigger'ın büyüklüğü ve süresinin birlikte etkisine bağlıdır.

### Table 1'den çıkarılan sınıflandırma

Makalede bir olayın yaşam döngüsü üç ayrı rolle incelenir:

1. **Trigger:** Kötü durumu başlatan geçici olay.
2. **Sustaining effect / amplification:** Trigger kalktıktan sonra kötü durumu sürdüren geri besleme.
3. **Mitigation:** Geri beslemeyi kırmak ve sistemi toparlamak için uygulanan müdahale.

Table 1 ve onu izleyen açıklamada retry en yaygın sustaining effect olsa da tek mekanizma değildir. Gözlenen örnekler arasında cascading overload, trafik veya iş kuyruğu büyümesi, hata yolunda aşırı loglama/pahalı işlem, lock contention, ZooKeeper veya leader-election churn, cascading server demotion, load rebalancing ve self-sustaining overload bulunur. Makalenin Twitter vakası ayrıca garbage collection'ın kapasiteyi düşüren bir amplification mekanizması olabileceğini gösterir.

Bu mekanizmalar iki geniş ailede düşünülebilir:

- **Workload amplification:** Retry, aşırı loglama veya yeniden dengeleme gibi davranışlarla yapılması gereken toplam iş artar.
- **Capacity-degradation amplification:** Garbage collection, lock contention veya election churn gibi davranışlarla yararlı işleme ayrılabilen etkin kapasite azalır.

Table 1'deki mitigation örnekleri arasında load shedding/throttling, retry veya başka politika değişiklikleri, kapasite artırma, restart/reboot, config rollback, hotfix, pahalı özelliği kapatma ve yükü yeniden yönlendirme bulunur. Mitigation bir sustaining effect değildir; başlamış geri beslemeyi kırmak için uygulanan düzeltici eylemdir. Her mitigation'ın kullanıcı maliyeti ayrıca değerlendirilmelidir: örneğin load shedding sistemi toparlarken bazı logical request'leri reddeder.

RetryCascade MVP'si yalnız **geçici sink kapasite düşüşü + retry kaynaklı workload amplification** mekanizmasını modeller. Diğer sustaining effect'ler literatür kapsamıdır; uygulanmış özellik gibi sunulmayacaktır.

### Okurken cevaplanacak sorular

1. Çalışmada kaç gerçek olay ve kaç kurum incelenmiş?
2. Gerçek olaylarda en sık görülen trigger türleri neler?
3. Sustaining effect hangi kaynakların tükenmesiyle oluşuyor?
4. Retry veya work amplification içeren hangi vakalar var?
5. Sistemler neden trigger ortadan kalkınca kendiliğinden toparlanamıyor?
6. Operatörler sistemi kurtarmak için hangi müdahaleleri uygulamış?
7. Çalışmanın gözlemsel verileri hangi sınırlılıklara sahip?
8. Bu çalışma RetryCascade modelindeki hangi varsayımları destekliyor?
9. Bu çalışma simülasyonumuzun hangi sayısal parametrelerini desteklemiyor?

### Okuma sonrası kısa özet

Makale, farklı görünen gerçek sistem kesintilerinde ortak bir desen bulur: geçici bir trigger, yükü artıran veya kapasiteyi azaltan bir amplification mekanizmasını başlatır ve sistem trigger kalktıktan sonra da kötü durumda kalır. Kamuya açık postmortem'lerden bulunan olaylar retry'nin en yaygın sustaining effect olduğunu, fakat queue growth, expensive error handling, contention ve coordination churn gibi başka mekanizmaların da bulunduğunu gösterir. Genişletilmiş çerçeve trigger'ları load spike ve capacity decrease; amplification'ı ise workload increase ve capacity degradation olarak sınıflandırır. Recovery çoğu olayda kendiliğinden gerçekleşmemiş, load shedding veya kapasite artırma gibi güçlü müdahaleler gerekmiştir. Makale ayrıca aynı başlangıç yükünde daha uzun veya daha güçlü trigger'ın sistemi metastable bölgeye taşıyabileceğini deneysel örneklerle gösterir. Bu çalışma RetryCascade'in araştırdığı mekanizmanın gerçek sistemlerde bulunduğunu destekler; bizim üç aşamalı simülatörümüzü veya sayısal parametrelerimizi doğrulamaz.

### RetryCascade için alınacak kararlar

- V1 yalnız `capacity-decreasing sink trigger + retry workload amplification` kombinasyonunu modeller.
- Ana deney mutlak bir üretim kapasitesini taklit etmek yerine normalize edilmiş bottleneck capacity ve `rho` taraması kullanır.
- `300 DB RPS + %90 cache hit + 3000 client RPS` yalnız vulnerable-state motivasyon örneğidir: `3000 * (1 - 0.90) = 300`. Kalibrasyon verisi veya varsayılan deney parametresi değildir.
- Cache ana modele eklenmez. Gerçek bir cache modeli hit/miss, cache state/refill ve zamanla değişen hit-rate gerektirir; ayrı gelecek çalışma olacaktır.
- `300 QPS sink / 280 QPS load / 1 s timeout / tek retry` varsa yalnız literature mechanism fixture olarak tutulur; ana deney matrisi değildir.
- `3000 run`, `3000 request` anlamına gelmez: ana tasarımda `2400 trigger + 600 no-trigger` bağımsız simülasyon koşusudur.
- Load shedding bir maliyetsiz başarı değildir; source-shed, abandonment ve success oranları recovery ile birlikte raporlanır.
- Garbage collection, contention, logging ve election churn gelecek amplification eklentileridir; MVP'de uygulanmış gibi sunulmaz.

### Anlamadığım veya tekrar bakacağım noktalar

- Bölüm 5'teki üç kontrollü uygulamanın parametreleri, canonical fixture yazılırken yeniden incelenecek.
- Cache deneyindeki sınıflandırma ufku bizim `H=600 s` operasyonel recovery ufkumuzla karıştırılmayacak.
- Kamuya açık postmortem'lerin ayrıntı düzeyi eşit değildir; yazarların gri işaretlediği vakalarda sustaining effect yorumu kesin değildir.

## 3. Altı yakın çalışma sonrası toplu karşılaştırma — 2026-08-24

Bu tablo tam sistematik literatür taraması değildir. Amacı, RetryCascade'in daha önce yapılmış geniş iddiaları yeniden yenilik gibi sunmasını önlemektir.

| Çalışma | Ana yöntem ve doğrulama | İşgal ettiği alan | RetryCascade için açık kalan dar soru |
|---|---|---|---|
| Huang vd., *Metastable Failures in the Wild*, OSDI 2022 | 11 kurumdan 22 olay; trigger/amplification sınıflandırması; üç kontrollü reproduction | Retry'nin yaygın sustaining effect olması; vulnerability × trigger şiddeti × süre | Tek başına policy tournament veya "retry metastability yaratır" iddiası yeni değildir. |
| Habibi vd., *MSF-Model*, SRDS 2024 | Timeout-retrial orbit queue ve distance divergence; PostgreSQL/Paxos prototipiyle boundary doğrulaması | Queue modeliyle metastability boundary tahmini ve gerçek prototip karşılaştırması | Yeni model iddiası yalnız aynı sınırı başka simülatörde çizmek olamaz. |
| Isaacs vd., *Analyzing Metastable Failures*, HotOS 2025 | CTMC → DES → emulator → stress-test toolchain | Çok katmanlı analiz/kalibrasyon yaklaşımı ve safe-region/recovery analizi | Emulator'da trigger sonrası retry oranının DES'ten yaklaşık `%20` yüksek kalmasının lifecycle nedenleri açık bir model-mismatch sorusudur. |
| Isaacs vd., *Formal Analysis of Metastable Failures in Software Systems*, 2025 | DSL, DES semantics ve DES verisiyle kalibre edilen CTMC; recovery/mitigation analizi | Queue, timeout ve retry parametre bölgeleri; recovery zamanı; throttling/autoscaling tahmini | Generic DES, recovery map veya CTMC calibration tek başına yeni değildir. |
| *RetryGuard*, 2025/2026 | AWS ve Istio testbedleri; backoff, jitter, retry budget/token-bucket ve adaptif kontrol karşılaştırması | Genel policy comparison ve rejection/latency tabanlı online retry controller | RetryCascade genel "hangi policy en iyi" veya yeni eşik controller'ı iddiası yapmamalıdır. |
| Farahbakhsh vd., *Characterizing Metastable Faults and Failures*, 2026 | Destabilizing component cycles, scheduling ve Nyx DSL; üç case study | Metastability'nin overload dışındaki yapısal/nedensel karakterizasyonu | Generic feedback-cycle veya scheduling'in önemli olduğu iddiası işgal edilmiştir. |

### Toplu hüküm

- V1'in aynı full-jitter altında budget etkisi, temiz bir staj araştırması ve tekrarlanabilir benchmark olarak kalır.
- Logical/attempt/cohort ayrımı doğru bilimsel muhasebe ve ucuz falsification sağlar; tek başına güçlü akademik yenilik değildir.
- Gerçek ölçüm olmadan belirsizlik taraması `certificate` veya production policy recommendation olarak adlandırılmaz.
- Queue/orbit sayılarının age/deadline geçmişini kaybetmesi ilginç tanısal sorudur; bağımsız ana rota olarak geniş teori ve doğrulama gerektirir.
- Seçilen akademik boşluk, gerçek testbedde timeout sonrası işin kısmi/gecikmeli cancellation davranışını ölçmek ve bu ölçümün held-out recovery tahminini iki uç cancellation baseline'ına göre iyileştirip iyileştirmediğini sınamaktır.

## 4. Cancellation in Systems — OSDI 2022

**Kaynak:** Sethi, U., Pan, H., Lu, S., Musuvathi, M. ve Nath, S. (2022). *Cancellation in Systems: An Empirical Study of Task Cancellation Patterns and Failures*.
**Bağlantı:** https://www.usenix.org/conference/osdi22/presentation/sethi

### RetryCascade açısından anlamı

- Cancellation, timeout oluştuğu anda fiziksel işin kesin ve anlık biçimde durması değildir.
- Cancel initiation, propagation ve fulfillment ayrı hata/gecikme noktalarıdır.
- Queue'da bekleyen iş ile başlamış veya kesilemez iş aynı cancellation davranışına sahip olmayabilir.
- V1'in `cancellation=false` seçimi açık ve kontrollü bir model varsayımı olarak korunmalıdır.
- Gelecek testbed uzantısı "cancellation iyidir" iddiasını değil; ölçülen cancellation gecikmesi/başarı oranının held-out recovery tahminine katkısını sınamalıdır.

## Yeni başlangıç kaynakları

- Habibi vd., *MSF-Model*: https://arxiv.org/abs/2309.16181
- Isaacs vd., *Analyzing Metastable Failures*: https://sigops.org/s/conferences/hotos/2025/papers/hotos25-106.pdf
- *Formal Analysis of Metastable Failures in Software Systems*: https://arxiv.org/abs/2510.03551
- *RetryGuard*: https://arxiv.org/abs/2511.23278
- Farahbakhsh vd., *Characterizing Metastable Faults and Failures*: https://arxiv.org/abs/2606.00942
