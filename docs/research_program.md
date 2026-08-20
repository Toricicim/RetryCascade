# RetryCascade Araştırma Programı

Bu belge aktif Gün 1–10 TODO listesi değildir. Güçlü bir sistemler yayınına uzanabilecek fikirleri, ancak önceki aşamanın kanıtı yeterliyse açılan koşullu araştırma fazları olarak saklar.

## Bugünkü karar

- **Aktif hedef — GO:** Deterministik ve doğrulanmış V1 simülatörü ile aynı full-jitter altında retry budget'ın recovery ve kullanıcı maliyetine etkisini inceleyen tekrarlanabilir staj araştırması.
- **Makale biçimli teknik rapor — GO:** Yöntem, negatif sonuçlar, belirsizlikler ve sınırlılıklar dahil article-shaped report.
- **Öğrenci sempozyumu veya workshop — CONDITIONAL GO:** Core, verification ve pilot kapıları geçer; sonuçlar sağlam ve sıradan olmayan bir trade-off gösterirse.
- **Güçlü sistemler konferansı veya dergisi — ŞİMDİ NO-GO:** Mevcut tek soyut simülasyon, dış doğrulama ve yeni teknik çözüm olmadan bu seviyede değildir.
- **Güçlü yayın için ayrı ikinci faz — CONDITIONAL GO:** Aşağıdaki kapılar sırayla geçerse değerlendirilebilir. Bu faz bugünkü Rust backlog'una eklenmeyecektir.

`NO-GO`, projenin kötü olduğu anlamına gelmez. Yalnızca mevcut kanıtın güçlü venue iddiası için yeterli olmadığını belirtir.

## Neden şimdi rushlamıyoruz?

Yakın çalışmalar retry kaynaklı metastability için kuyruk modelleri, DES, CTMC, recovery analizi ve gerçek sistem doğrulaması sunuyor. Bu nedenle yalnız "dört policy çalıştırdık ve boundary çizdik" katkısı güçlü yayın için yeterli değildir.

Güçlü venue değerlendirmeleri, önemli bir problem yanında önceki çalışmalardan açık bir ilerleme, ilgi çekici bir çözüm ve çözümün pratik yararını gösteren kanıt bekler. Bizde şu anda önce doğrulanması gereken simülasyon çekirdeği vardır; henüz yeni çözüm veya dış doğrulama yoktur.

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

### P2 — Pilot ve protokolü dondurma

**Durum:** CONDITIONAL; P1 sonrasında

Pilot seed'leri ana analizden ayrı tutulur. Pilot yalnız çalışma süresini, recovery/non-recovery geçiş bölgesini ve gereken grid/seed hassasiyetini belirler. Beş kaba `rho` noktası ve hücre başına 30 seed'in `rho_50` için yeterli olduğu varsayılmaz.

**Go:** Birden fazla hücrede tekrarlanabilir recovery/non-recovery geçişi ve açıklanmaya değer recovery–kullanıcı-maliyeti etkisi vardır.

**Kill:** Sonuç yalnız "retry azaltılınca ek yük azalır" düzeyindeyse veya küçük semantik/eşik değişikliklerinde kayboluyorsa güçlü yayın hedefi zorlanmaz. Proje güçlü staj raporu ve artifact olarak tamamlanır.

### P3 — Ana simülasyon çalışması

**Durum:** CONDITIONAL; P2 sonrasında

Süreye özel recovery eğrileri/boundary'ler, hücre bazlı paired etkiler, kullanıcı maliyetleri ve recovery tanımı/horizon/service-CV/timeout duyarlılıkları raporlanır. Extrapolation yapılmaz; crossing yoksa boundary censored/not estimable olarak gösterilir.

Bu aşamanın sonunda iki tür bulgu güçlü yayın için ikinci fazı gerekçelendirebilir:

1. Yaygın retry rehberine karşı, geniş bir parametre bölgesinde tekrarlanan keskin bir karşı örnek veya recovery–kullanıcı-maliyeti crossover'ı.
2. Basit kapasite kurallarından daha iyi held-out boundary tahmini vaat eden, sonuçtan sonra uydurulmamış bir ölçeksiz düzenlilik.

İkisi de çıkmazsa publication scope büyütülmez.

### P4 — Tek gerçek sistemde feasibility

**Durum:** FUTURE / CONDITIONAL

Kontrollü, read-like ve instrument edilmiş bir servis + backend üzerinde logical request ile fiziksel attempt eşlenir; geçici downstream kapasite kaybı tekrarlanabilir biçimde üretilir. Amaç bütün PostgreSQL sistemlerini tahmin etmek değil, simülasyondaki mekanizmanın ölçülebilir bir gerçek testbedde görülüp görülmediğini sınamaktır.

**Kill:** Attempt parentage gözlenemiyorsa, olay tekrarlanabilir değilse veya standart kontrol boşluğu zaten kapatıyorsa strong-paper genişlemesi durur.

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
- *Formal Analysis of Metastable Failures in Distributed Systems*: https://arxiv.org/abs/2510.03551
- OSDI 2026 değerlendirme ölçütleri: https://www.usenix.org/conference/osdi26/call-for-papers

## Kapsam koruma kuralı

Bu belgedeki `FUTURE` ve `CONDITIONAL` maddeler aktif TODO değildir. `docs/todo/README.md` ve Gün 5–10 dosyaları V1 için otorite olmaya devam eder. P0 bitmeden gerçek DB, ML/controller, yeni fault family, cache, GC, Parquet veya PostgreSQL sonuç deposu core'a eklenmez.
