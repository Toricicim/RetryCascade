# RetryCascade Karar Günlüğü

## 2026-08-24 — Altı makale ve üç ajan sonrası akademik uzantı kararı

### Karar

1. V1'in ana araştırma sorusu, H1'i, `cancellation=false` semantiği ve Gün 5–10 backlog'u değiştirilmedi.
2. P0 sonrasında logical/attempt/cohort metrik denetimi zorunlu değerlendirme olarak eklendi; bağımsız ana akademik katkı sayılmadı.
3. En fazla beş günlük assumption-sensitivity robustness screen, pahalı uzantıya geçmeden önce ucuz karar kapısı olarak kabul edildi.
4. `Robust certificate`, `recommend/reject` ve üretim politika önerisi terminolojileri gerçek ölçüm/formal güvence olmadan reddedildi.
5. Queue/orbit history-sufficiency fikri bağımsız ana rota olarak şimdilik `NO-GO`; gerekirse tanısal yan deney olarak tutuldu.
6. Tek ana akademik uzantı olarak measured partial cancellation + held-out recovery validation seçildi.
7. Gerçek testbed uzantısı V1'i geriye dönük değiştirmeyecek; ayrı `model_version` ve protokol gerektirecek.

### Gerekçe

Yakın çalışmalar retry metastability, queue/orbit boundary tahmini, DES/CTMC kalibrasyonu, gerçek AWS/Istio politika karşılaştırması ve yapısal metastability karakterizasyonu alanlarının önemli bölümünü kapatmaktadır. Buna karşılık HotOS 2025 simülatör ve emulator arasında trigger sonrası retry oranında açıklanmamış bir fark raporlar; cancellation çalışmaları da timeout sonrası işin durdurulmasının uygulamada karmaşık olduğunu gösterir. Ölçülen cancellation semantiğinin held-out recovery tahminine katkısı, açık baselines ve başarısız olabilecek dar bir dış doğrulama sorusu sunar.

### Stop kuralı

P0 geçmezse, beş günlük screen yeni mekanik sinyal üretmezse veya testbedde attempt parentage/cancellation güvenilir ölçülemezse akademik kapsam yeni özelliklerle zorlanmayacak; proje güçlü staj artifact'ı ve teknik rapor olarak tamamlanacaktır.

## 2026-08-19 — Akademik hedef ve güçlü yayın rotası

### Karar

1. Ana araştırma sorusu, aynı full-jitter altında retry budget'ın incremental recovery etkisini ve kullanıcı maliyetini ölçmek olarak daraltıldı.
2. Eski `retry_budget vs immediate` H1'i budget etkisini izole etmediği için kaldırıldı.
3. Eski saf jitter H2'si, deterministic ve randomized bekleme süreleri mean-matched olmadığı için birincil nedensel hipotez olmaktan çıkarıldı.
4. Dört policy'nin tamamı uygulanan politika paketleri olarak betimleyici benchmarkta kalacak.
5. Güçlü sistemler yayını mevcut kapsam için `NO-GO`, core sonrası kapılı ikinci araştırma fazı için `CONDITIONAL GO` olarak kaydedildi.
6. Gerçek sistem, yeni controller ve sistemler arası prediction fikirleri aktif Gün 5–10 backlog'una eklenmeyecek.

### Gerekçe

Yakın çalışmalar retry metastability için DES, queue/CTMC modelleri, recovery boundary ve gerçek sistem doğrulaması sunuyor. Exact policy kombinasyonunun daha önce aynı dosyada çalıştırılmamış olması tek başına bilimsel yenilik değildir. Mevcut projede önce deterministik çekirdek, temiz nedensel kontrol, kullanıcı maliyeti ve pilot/main ayrımı tamamlanmalıdır.

### Yeniden değerlendirme zamanı

- P0: V1 çekirdeği ve doğruluk kapısı geçince,
- P2: pilotta recovery/non-recovery boundary ve sağlam bir trade-off görülünce,
- P4: tek gerçek testbed feasibility sonucu alınınca.

Bu kapılardan biri başarısız olursa yayın hedefini özellik ekleyerek zorlamak yerine güçlü staj raporu/artifact teslim edilir.
