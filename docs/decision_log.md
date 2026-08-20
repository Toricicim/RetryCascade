# RetryCascade Karar Günlüğü

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
