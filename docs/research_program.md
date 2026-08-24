# RetryCascade Araştırma Programı

Bu belge aktif Gün 1–10 TODO listesi değildir. Güçlü bir sistemler yayınına uzanabilecek fikirleri, ancak önceki aşamanın kanıtı yeterliyse açılan koşullu araştırma fazları olarak saklar.

## Bugünkü karar

- **Aktif hedef — GO:** Deterministik ve doğrulanmış V1 simülatörü ile aynı full-jitter altında retry budget'ın recovery ve kullanıcı maliyetine etkisini inceleyen tekrarlanabilir staj araştırması.
- **Makale biçimli teknik rapor — GO:** Yöntem, negatif sonuçlar, belirsizlikler ve sınırlılıklar dahil article-shaped report.
- **Öğrenci sempozyumu veya workshop — CONDITIONAL GO:** Core, verification ve pilot kapıları geçer; sonuçlar sağlam ve sıradan olmayan bir trade-off gösterirse.
- **Güçlü sistemler konferansı veya dergisi — ŞİMDİ NO-GO:** Mevcut tek soyut simülasyon, dış doğrulama ve yeni teknik çözüm olmadan bu seviyede değildir.
- **P0 sonrası zorunlu değerlendirme — GO, P0 TARAFINDAN BLOKE:** Logical/attempt/cohort metrik denetimi, ana simülasyon sonuçlarının kullanıcı değeri ile fiziksel işi karıştırıp karıştırmadığını sınayacaktır. Bu denetim tek başına akademik katkı sayılmayacaktır.
- **Beş günlük ucuz karar kapısı — CONDITIONAL GO:** Küçük bir assumption-sensitivity robustness screen, daha pahalı akademik uzantıya yatırım yapmaya değip değmediğini sınayacaktır. Bu bir sertifika veya üretim öneri sistemi değildir.
- **Tek ana akademik uzantı — FUTURE / CONDITIONAL:** Kontrollü ve overload olmayan deneylerden ölçülen partial cancellation davranışının, held-out kapasite kaybı deneylerindeki recovery'yi iki basit cancellation baseline'ından daha iyi tahmin edip etmediği sınanacaktır. Bu faz bugünkü Rust backlog'una eklenmeyecektir.

`NO-GO`, projenin kötü olduğu anlamına gelmez. Yalnızca mevcut kanıtın güçlü venue iddiası için yeterli olmadığını belirtir.

## Neden şimdi rushlamıyoruz?

Yakın çalışmalar retry kaynaklı metastability için kuyruk modelleri, DES, CTMC, recovery analizi ve gerçek sistem doğrulaması sunuyor. Bu nedenle yalnız "dört policy çalıştırdık ve boundary çizdik" katkısı güçlü yayın için yeterli değildir.

Güçlü venue değerlendirmeleri, önemli bir problem yanında önceki çalışmalardan açık bir ilerleme, ilgi çekici bir çözüm ve çözümün pratik yararını gösteren kanıt bekler. Bizde şu anda önce doğrulanması gereken simülasyon çekirdeği vardır; henüz yeni çözüm veya dış doğrulama yoktur.

## Son literatür turundan çıkan kapsam kararı

Altı yakın çalışma sonrası aşağıdaki geniş iddialar akademik boşluk olarak kullanılmayacaktır:

- Retry'nin metastable failure üretebildiği zaten gösterilmiştir.
- Queue/orbit modeliyle recovery boundary tahmini ve DES/CTMC kalibrasyonu zaten çalışılmıştır.
- Backoff, jitter, retry budget ve adaptif retry kontrolünün genel karşılaştırması gerçek AWS/Istio ortamlarında yapılmıştır.
- Metastability'nin yapısal geri besleme döngüleri ve scheduling üzerinden nedensel karakterizasyonu ayrı bir çalışma hattında ele alınmıştır.

Bu nedenle RetryCascade'in akademik yükseltmesi "dört politikayı aynı grafikte karşılaştırmak" olmayacaktır. Seçilen dar boşluk, timeout sonrası fiziksel işin gerçek sistemde tamamen devam eden veya kusursuzca iptal edilen iki uç davranış arasında kalabilmesi ve bu gizli lifecycle semantiğinin recovery tahminini değiştirebilmesidir.

| Aday | Statü | Gerekçe |
|---|---|---|
| Logical/attempt/cohort metrik denetimi | P0 sonrası gerekli değerlendirme | Dürüst muhasebe ve ucuz falsification sağlar; tek başına yeni bilimsel katkı değildir. |
| Assumption-sensitivity robustness screen | Koşullu, en fazla beş günlük karar kapısı | Yalnız hangi varsayımlarda yorumun değiştiğini gösterir; gerçek ölçüm olmadan `certificate` veya öneri sistemi değildir. |
| Queue/orbit history-sufficiency | Şimdilik NO-GO / tanısal yedek | Yaş ve deadline geçmişinin önemli olması beklenir; bağımsız ana iddia için teori ve dış doğrulama yükü fazladır. |
| Measured partial cancellation + held-out validation | FUTURE / tek ana akademik uzantı | Gerçek ölçüm, açık baselines, held-out test ve başarısız olabilecek dar bir tahmin iddiası sunar. |

P0 sonrası karar kapısının ayrıntılı sözleşmesi için [post-core akademik uzantı kapısı](research_extension_gate.md) kullanılacaktır. Bu belge aktif Gün 1–10 backlog'u değildir.

## Aşamalı karar kapıları

### P0 — V1 çekirdeği ve doğruluk

**Durum:** ACTIVE

**Teslim:** `resolved config → deterministik DES → kapanan lifecycle/conservation ledger → dört policy → recovery summary`.

**Zorunlu kanıt:** Aynı config ve seed aynı sonucu üretir; event sırası sabittir; queue/work ledger kapanır; küçük elle hesaplanan trace'ler geçer.

**Kill:** Bu doğruluk kapısı geçmezse bilimsel deney veya yayın genişlemesi yapılmaz. Teknik çekirdek düzeltilir.

### P1 — Bilimsel sağlamlaştırma

**Durum:** CONDITIONAL; P0 sonrasında

**Teslim:**

- `max_attempts=1` ile trigger + no-retry nedensel kontrolü,
- aynı full-jitter altında budget kapalı/açık karşılaştırması,
- ayrı exponential-service basit `M/M/1/K` bilinen-sonuç fixture'ı,
- common-random-number eşitlik kontrolleri,
- recovery ile unique on-time success/abandonment/amplification maliyetlerinin birlikte raporlanması,
- küçük, yeniden üretilebilir CLI/artifact smoke yolu.

`M/M/1/K` fixture, ana lognormal modelin gerçekçiliğini doğrulamaz; yalnız simülasyon motorunun bilinen basit bir durumda doğru davranıp davranmadığını sınar.

### P2 — Pilot, metrik denetimi ve ucuz robustness screen

**Durum:** CONDITIONAL; P1 sonrasında

Pilot seed'leri ana analizden ayrı tutulur. Pilot yalnız çalışma süresini, recovery/non-recovery geçiş bölgesini ve gereken grid/seed hassasiyetini belirler. Beş kaba `rho` noktası ve hücre başına 30 seed'in `rho_50` için yeterli olduğu varsayılmaz.

Pilotla birlikte aynı run'lardan şu dört bakış ayrı üretilir:

- fiziksel attempt completion,
- deadline içindeki benzersiz logical success,
- late/duplicate yararsız fiziksel iş,
- `pre-trigger`, `during-trigger` ve `post-trigger` arrival kohortlarının başarısı.

Ardından önceden yazılmış küçük varsayım aralıklarında arrival/service parametreleri, timeout/deadline ve recovery tanımı için en fazla beş günlük `assumption-sensitivity robustness screen` yapılır. Çıktılar yalnız `tested range içinde stable`, `tested range içinde unstable` veya `ayırt edilemiyor` olarak adlandırılır. `Certificate`, `recommend/reject` ve üretim tavsiyesi dili kullanılmaz.

**Go:** Birden fazla hücrede tekrarlanabilir recovery/non-recovery geçişi vardır ve metrik/kohort bakışı veya savunulabilir küçük varsayım değişiklikleri politika etkisinin yönünü, büyüklüğünü ya da recovery yorumunu açıklanmaya değer biçimde değiştirir. Bu sinyal yalnız daha ayrıntılı araştırmayı gerekçelendirir; tek başına akademik yenilik kanıtı değildir.

**Kill:** Sonuç yalnız "retry azaltılınca ek yük azalır" düzeyindeyse; değişim yalnız keyfî/gerçekçi olmayan aralıklarda oluşuyorsa; Monte Carlo belirsizliği etkiden büyükse; ya da bulgu recovery tanımındaki küçük ve savunulabilir değişikliklerde kayboluyorsa güçlü yayın hedefi zorlanmaz. Proje güçlü staj raporu ve artifact olarak tamamlanır.

### P3 — Ana simülasyon çalışması

**Durum:** CONDITIONAL; P2 sonrasında

Süreye özel recovery eğrileri/boundary'ler, hücre bazlı paired etkiler, kullanıcı maliyetleri ve recovery tanımı/horizon/service-CV/timeout duyarlılıkları raporlanır. Extrapolation yapılmaz; crossing yoksa boundary censored/not estimable olarak gösterilir.

Bu aşamanın sonunda iki tür bulgu güçlü yayın için ikinci fazı gerekçelendirebilir:

1. Yaygın retry rehberine karşı, geniş bir parametre bölgesinde tekrarlanan keskin bir karşı örnek veya recovery–kullanıcı-maliyeti crossover'ı.
2. Basit kapasite kurallarından daha iyi held-out boundary tahmini vaat eden, sonuçtan sonra uydurulmamış bir ölçeksiz düzenlilik.

İkisi de çıkmazsa publication scope büyütülmez.

### P4 — Measured partial cancellation ile held-out testbed validation

**Durum:** FUTURE / CONDITIONAL; P0–P2 geçmeden aktif değildir

**Ana araştırma sorusu:** Tek, instrument edilmiş ve read-like bir RPC testbedinde; overload olmayan kontrollü calibration deneylerinden ölçülen cancellation gecikmesi ve başarı oranını kullanan DES, daha önce görülmemiş geçici kapasite kaybı deneylerinde unique on-time success, yararsız fiziksel iş ve recovery sonucunu iki uç baseline'dan daha doğru tahmin eder mi?

**Baselines:**

1. Timeout sonrası hiçbir attempt iptal edilmez.
2. Timeout sonrası bütün uygun attempt'ler anında ve kusursuz iptal edilir.
3. Kontrollü testbed ölçümünden alınan gecikmeli/kısmi cancellation modeli.

Logical request ile bütün fiziksel attempt'ler parentage ve monotonic timestamp'lerle eşlenir. Cancellation calibration koşuları trigger grid'inden ayrı tutulur; model parametreleri held-out kapasite kaybı sonuçları görülmeden dondurulur. V1'in `cancellation=false` semantiği geriye dönük değiştirilmez; uzantı ayrı `model_version` ve ayrı protokol kullanır.

**Go/acceptance:** Ölçülen partial-cancellation modeli, held-out koşullarda iki uç baseline'ı unique on-time success hatası, wasted-work hatası ve recovery sınıflandırma hatasının önceden seçilmiş en az birinde tekrarlanabilir biçimde geçer; diğer metriklerde açıklanamayan ciddi regresyon üretmez.

**Kill:** Attempt parentage veya cancellation'ın gerçek durma zamanı güvenilir ölçülemiyorsa; kapasite kaybı tekrarlanabilir değilse; calibration için held-out trigger sonuçlarına bakmak gerekiyorsa; ya da ölçülen model iki uç baseline'ı geçmiyorsa akademik tahmin iddiası bırakılır. Sonuç yine testbed-sınırlı negatif bulgu olarak raporlanabilir; güçlü yayın sözü verilmez.

### P5 — Güçlü yayın rotasını seçme

**Durum:** FUTURE / yalnız P4 sonrasında

Sonuç görülmeden rota seçilmeyecektir:

- **Keskin karşı örnek rotası:** Karşı örneği iki mekanik olarak farklı gerçek sistemde, held-out ayarlarda ve temiz ablation'larla doğrula. Controller zorunlu değildir.
- **Sistemler arası yasa rotası:** En az iki discovery/calibration sistemi ve dokunulmamış üçüncü sistemde held-out prediction gerekir. Üç sistem bir venue kuralı değil; "cross-system transfer" iddiasının mantıksal bedelidir.
- **Yeni controller rotası:** Yalnız statik politikaların farklı bölgelerde kazandığı ve çevrimiçi seçim için gerçek bir boşluk bulunduğu gösterilirse açılır. Yeni controller; mevcut adaptive retry yöntemlerine karşı safety, overhead, stability, ablation ve en az iki farklı gerçek sistem kanıtı ister.

Yeni teori/formal yöntem rotası, kuyruk teorisi veya formal methods konusunda aktif ortak olmadan bu proje için önerilmemektedir.

## Süre gerçeği

- V1 core: yaklaşık `30–50` odaklı saat.
- Core sonrası güçlü bir simülasyon/teknik rapor paketi: yaklaşık `20–35+` saat ve deney/yazım süresi.
- Küçük tek-testbed feasibility: altyapıya göre haftalar.
- Reviewer-proof güçlü sistemler yayını: yeni başlayan tek araştırmacı için birkaç haftalık ek değil; aktif hoca/ortak ve altyapıyla dahi aylar süren ayrı bir araştırma programıdır. Çok sistemli controller veya transfer çalışması `6–12+ ay` sürebilir; kesin süre veya kabul garantisi yoktur.

## Yakın çalışma kontrol listesi

İkinci fazdan önce şu cümle somut biçimde tamamlanmalıdır:

> "MSF-Model, Formal Analysis, Metafor/HotOS yaklaşımı ve güncel adaptive retry çalışmalarından farklı olarak RetryCascade ..."

Bu boşluk yalnız "Rust kullandı", "dört policy'yi aynı grafikte gösterdi" veya "exact kombinasyon daha önce yok" ile doldurulamaz.

Başlangıç kaynakları:

- Bronson vd., *Metastable Failures in Distributed Systems* (HotOS 2021): https://doi.org/10.1145/3458336.3465286
- Huang vd., *Metastable Failures in the Wild* (OSDI 2022): https://www.usenix.org/system/files/osdi22-huang-lexiang.pdf
- Habibi vd., *MSF-Model*: https://arxiv.org/abs/2309.16181
- Isaacs vd., *Analyzing Metastable Failures* (HotOS 2025): https://sigops.org/s/conferences/hotos/2025/papers/hotos25-106.pdf
- *Formal Analysis of Metastable Failures in Distributed Systems*: https://arxiv.org/abs/2510.03551
- *RetryGuard*: https://arxiv.org/abs/2511.23278
- Farahbakhsh vd., *Characterizing Metastable Faults and Failures*: https://arxiv.org/abs/2606.00942
- Sethi vd., *Cancellation in Systems* (OSDI 2022): https://www.usenix.org/conference/osdi22/presentation/sethi
- OSDI 2026 değerlendirme ölçütleri: https://www.usenix.org/conference/osdi26/call-for-papers

## Kapsam koruma kuralı

Bu belgedeki `FUTURE` ve `CONDITIONAL` maddeler aktif TODO değildir. `docs/todo/README.md` ve Gün 5–10 dosyaları V1 için otorite olmaya devam eder. P0 bitmeden gerçek DB, ML/controller, yeni fault family, cache, GC, Parquet veya PostgreSQL sonuç deposu core'a eklenmez.
