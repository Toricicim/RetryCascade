# RetryCascade Post-Core Akademik Uzantı Kapısı

> **ACTIVE TODO DEĞİLDİR.** Bu belge yalnız Gün 10 stop gate'i ve P0 doğruluk kapısı geçtikten sonra kullanılır. Gün 5–10'a yeni policy, cancellation, snapshot, testbed veya uncertainty özelliği eklemez.

## Amaç

Pahalı bir akademik uzantıya başlamadan önce, mevcut V1 artifact'lerinin daha ileri araştırmayı gerekçelendiren bir sinyal taşıyıp taşımadığını beş günlük ucuz bir falsification ile sınamak.

Bu kapının kendisi makale katkısı değildir. Sonuç yalnız şu kararlardan birini üretir:

- `STOP — güçlü staj artifact'ı ve teknik rapor olarak tamamla`,
- `GO — dar assumption-sensitivity analizini sürdür`,
- `GO — mentor ve altyapı varsa measured-cancellation testbed feasibility tasarla`.

## Önkoşullar

- `resolved config → deterministic run → closed ledger → four policies → run_summary.json` zinciri geçer.
- Logical request ile bütün fiziksel attempt'ler eşlenebilir.
- Unique on-time success, late, duplicate, abandonment ve amplification sayaçları elle doğrulanmış fixture'larda doğrudur.
- Full-jitter ve retry-budget kolları common-random-number sözleşmesini geçer.
- Boundary'ye yakın en az birkaç pilot hücrede hem recovery hem non-recovery gözlenir.

Önkoşullardan biri geçmezse bu çalışma başlatılmaz; V1 düzeltilir.

## Gün F1 — Muhasebe ve kohort sözleşmesi

**Tahmin:** 5–7 odaklı saat.

- Fiziksel attempt completion ile deadline-içi benzersiz logical success'ı ayrı raporla.
- Late ve duplicate completion'ların tükettiği fiziksel işi ayrı muhasebeleştir.
- Logical arrival'ları `pre-trigger`, `during-trigger` ve `post-trigger` kohortlarına ayır.
- Her logical request ve attempt'in tek anlamlı terminal sınıfa kapandığını test et.

**Acceptance:** Conservation/lifecycle ledger bütün fixture ve seçilen run'larda kapanır.

**Kill:** Ledger kapanmıyorsa araştırma yorumu yapılmaz.

## Gün F2 — Küçük eşlenmiş deney

**Tahmin:** 4–6 odaklı saat.

- Boundary çevresinden az sayıda `rho × trigger duration` hücresi seç.
- Birincil olarak aynı full-jitter altında retry budget kapalı/açık kollarını paired seed'lerle çalıştır.
- Dört policy paketini yalnız betimleyici bağlam olarak göster.
- Hücre ve seed seçimini sonuçlara bakmadan kaydet.

**Acceptance:** Seçilen küçük grid en az bir recovery ve bir non-recovery bölgesi içerir; paired trace sözleşmesi doğrulanır.

**Kill:** Bütün hücreler aynı sınıftaysa önce pilot grid düzeltilir; fark varmış gibi yorum yapılmaz.

## Gün F3 — Metrik ve cohort audit

**Tahmin:** 4–6 odaklı saat.

Şu bakışların politika etkisini veya recovery yorumunu değiştirip değiştirmediğini sınar:

- total physical completion,
- unique on-time logical success,
- during-trigger cohort success,
- post-trigger cohort success,
- late/duplicate wasted work,
- ana operational recovery sınıfı.

**Acceptance:** Her paired etki, seed belirsizliği ve açık payda ile raporlanır.

**Bilimsel sınır:** Bir sıralama farkı bulunması tek başına yenilik değildir; deadline-aware metriklerin farklı olması büyük ölçüde tanımsal olabilir.

## Gün F4 — Assumption-sensitivity robustness screen

**Tahmin:** 5–7 odaklı saat.

Önceden yazılmış küçük aralıklarda yalnız şu varsayımlar taranır:

- arrival rate,
- service-rate ölçeği veya service CV,
- timeout/deadline,
- recovery horizon/threshold sensitivity.

Bu aralıklar ampirik confidence interval gibi sunulmaz; `model-assumption perturbation range` olarak etiketlenir. Gerçek ölçüm yokken `certificate`, `policy recommendation`, `recommend/reject` veya production guidance dili kullanılmaz.

İzin verilen çıktılar:

- `stable within tested assumption range`,
- `unstable within tested assumption range`,
- `not distinguishable`.

**Kill:** Karar yalnız keyfî/gerçekçi olmayan aralıklarda değişiyorsa, Monte Carlo belirsizliği etkiden büyükse veya sonuç recovery tanımındaki küçük savunulabilir değişikliklerde kayboluyorsa robustness rotası kapanır.

## Gün F5 — Karar notu

**Tahmin:** 4–6 odaklı saat.

Tek sayfalık karar notu şu soruları yanıtlar:

1. Kullanıcı-merkezli metrik ile fiziksel attempt metriği arasında tekrarlanabilir ve maddi bir yorum farkı var mı?
2. Bu fark yalnız tanımın mekanik sonucu mu, yoksa late/duplicate uncancelled work gibi izlenebilir bir mekanizma gösteriyor mu?
3. Politika etkisi test edilen makul varsayım aralığında stable mı, unstable mı, ayırt edilemiyor mu?
4. Uncancelled wasted work, gerçek testbedde cancellation davranışını ölçmeye yatırım yapmayı gerekçelendirecek kadar merkezi mi?
5. Testbed için zaman, mentor ve tracing altyapısı var mı?

## Nihai karar kuralları

### STOP

Aşağıdakilerden biri yeterlidir:

- Sonuç yalnız "retry azalınca amplification azalır" düzeyindedir.
- Metrik farkı yalnız tanımsaldır ve yeni bir mekanizma göstermemektedir.
- Politika etkileri Monte Carlo belirsizliğinden küçük veya eşiklere aşırı duyarlıdır.
- Savunulabilir assumption range içinde anlamlı hassasiyet yoktur.
- Testbed için aktif mentor veya yeterli süre yoktur.

Bu durumda proje deterministik, tekrarlanabilir ve sınırları açık güçlü bir staj artifact'ı/teknik rapor olarak tamamlanır.

### CONDITIONAL GO — Robustness analizi

Yalnız politika etkisi savunulabilir assumption range içinde tekrarlanabilir biçimde işaret veya maddi büyüklük değiştiriyorsa açılır. Gerçek ölçüm olmadan bu analiz hâlâ simulation sensitivity study'dir; production recommendation değildir.

### FUTURE GO — Measured cancellation testbed

Yalnız uncancelled late/duplicate work ana mekanizma olarak görünür, attempt parentage ölçülebilir, trigger tekrarlanabilir ve yeterli zaman/mentor varsa tasarlanır.

Ana falsifiable claim:

> Kontrollü ve overload olmayan calibration deneylerinden ölçülen cancellation gecikmesi ve başarı oranını kullanan DES, cancellation-yok ve anlık-kusursuz-cancellation baselines'ına göre held-out geçici kapasite kaybı deneylerinde unique on-time success, wasted work ve recovery sonucunu daha doğru tahmin eder.

Bu iddia tek testbed ve önceden dondurulmuş deney zarfıyla sınırlıdır; üretim sistemlerinin genel tahmincisi olduğu söylenmez.
