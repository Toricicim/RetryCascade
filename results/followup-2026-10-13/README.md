# 13 Ekim 2026 keşif sonuçları

Doğrulanmış pre-release study'den public snapshot. 630 koşu, beş yeni keşif
seed'i (910004–910008), 27.090 feature satırı. Confirmation bankası kullanılmadı.
Ham artifact'lar bu klasörde bulunmaz. Source plan/binary/lock/analysis hash'leri
[manifest.json](manifest.json), bütün hücre ortalamaları ve frozen follow-up
protokolü [snapshot.json](snapshot.json) içindedir. Bunlar daha sonraki packaging
commit'ini değil, gerçekten koşmuş original binary ve study'yi tanımlar.

## Kullanıcı kaybı ile toparlanma farklı şeyler

Nominal rho=.575, IID pre-admission p=.10, 10s yarım Sink kapasitesi, strict RPC:

| Kol | Incident arrival başarı | İlk60s post arrival başarı | Recovery600s |
|---|---:|---:|---:|
| Jitter + budget10 | %29,18 | %98,34 | 5/5 |
| Full jitter | %20,27 | %0,00 | 0/5 |
| No retry | %64,24 | %89,88 | 5/5 |

Budget bu hücrede sonraki kullanıcıları kurtarırken arıza anında daha fazla
kullanıcı kaybediyor. 5/5 için eligible-seed Wilson95% alt sınırı yaklaşık
.566; 0/5 için üst sınır .434. Bunlar küçük örneklemli, keşifsel seed ortalamalarıdır.

![Mutlak cohort başarısı](incident_outcomes.png)

## İstemci sözleşmesi önemli bir varsayım

rho=.65, p=.10, 10s trigger altında budget'ın incident başarısı strict RPC'de
%14,46; deadline'a kadar cevabı kabul edip quota reddinde mevcut işi bekleyen
varyantta %82,89. No-retry da strict %26,34'ten accept varyantında %89,48'e
çıkıyor. Bu gerçek servis üzerinde uygulanmış iyileştirme değil, modelde
kullanıcının hangi cevabı kabul ettiğini değiştiren yapısal sensitivity'dir.

## Zaman serisi

![Betimleyici indeks ve operasyonel bileşenler](recovery_curves.png)

İndeks 100 bütün kullanıcıların başarılı olması değildir. Policy'nin kendi
baseline'ına normalize edilir; resmi exact Rust recovery kuralı ayrı kalır.
EWMA/delta/headroom/AUC geçmiş bin'lere dayanır; retrospective cohort canlı
feature değildir. Figürler p=.10, 10s reference incident, strict RPC hücrelerini
gösterir. Gölge seed min–max, güven aralığı değildir.

## Yeniden çalıştırma

Repo kökünde `make setup && make followup`. Kullanım [ana README](../../README.md),
model sözleşmesi [MODEL.md](../../MODEL.md) içindedir. Başka platform/binary'de
floating-point örneklemenin bit düzeyinde eşitliği garanti edilmez. Bu snapshot
tam raw replay archive değildir; yeni çalışma yeni provenance üretir.

Low-load IID sonuçları büyük ölçüde basit attempt/kota hesabıyla açıklanabilir.
Evrensel üstünlük, özgün akademik katkı veya production tahmini çıkarılmaz.
Gerçek testbed, correlation ve cancellation bu sonuçlara dahil değildir.
