# Gün 2 TODO — Semantiği ve Protokolü Kilitleme

## Günün amacı

Kodlamaya başlamadan önce simülasyonun bütün davranışlarını ve deney parametrelerini kesinleştirmek. Sonuçlar görüldükten sonra yapılan her semantik değişiklik yeni `model_version` ve `protocol_hash` gerektirecek.

## Durum

`TAMAMLANDI — SEMANTİK VE PROTOKOL KİLİTLENDİ`. Tam resolved-run schema ve golden fixture, karar çalışması değil implementasyon olduğu için Gün 3'ün ilk işi olarak devredildi; Rust simülasyon kodu bu teslim tamamlanmadan başlamayacak.

## Gün 3 öncesi zorunlu semantik açıklar

- [x] `LogicalDeadline` olayını, başlangıcını ve scheduler priority’sini tanımla.
- [x] Aynı tick sırasını kilitle: completion → attempt timeout → logical deadline → trigger → retry ready → arrival → queue observation.
- [x] `RetryReady == LogicalDeadline` durumunda deadline’ın kazanacağını yaz ve test senaryosunu tanımla.
- [x] Source queue doluysa attempt’in `queue_rejected(source)` olacağını; Source admission olmadığı için timeout başlamayacağını yaz.
- [x] `attempts_exhausted`, `deadline_abandoned`, late ve duplicate muhasebesini tek anlamlı state tablosuna bağla.
- [x] No-trigger koşulunu explicit `trigger.enabled: false` ile temsil et.
- [x] Horizon’ları trigger bitişine göre süre olarak tanımla: `run_end = trigger_end + post_trigger_duration`.
- [x] Recovery pencerelerini `trigger_end` anına hizala ve pencere sınırındaki completion kuralını yaz.
- [x] Queue recovery metriğini tanımla: 30 saniyelik exact time-weighted mean waiting occupancy; 100 ms sample yalnız debug/görselleştirme.
- [x] Service completion süresinde integer ceiling yuvarlamasını ve stale epoch event sayacını tanımla.
- [x] Retry budget’ın run-global olduğunu ve completion/refill’in aynı tick RetryReady’den önce işlendiğini yaz.
- [x] H1/H2 için tek authoritative metni protokol ile `research_question.md` arasında eşleştir.
- [x] Lognormal ana model için `M/M/1/K` iddiasını kaldır; analitik test yapılacaksa ayrı exponential-service fixture tanımla.
- [ ] Python’ın üreteceği tam `resolved_run_config.json` şemasını ve golden örneğini tamamla. **Gün 3'ün ilk geliştirme işi olarak devredildi.**

## Tamamlananlar

- [x] OSDI 2022 ilk okumasının notlarını tamamla.
- [x] Trigger, sustaining effect ve mitigation ayrımını belgele.
- [x] Retry dışındaki amplification mekanizmalarını gelecek çalışma olarak kaydet.
- [x] `300 DB RPS / %90 cache hit / 3000 client RPS` örneğinin yalnız motivasyon olduğunu belirt.
- [x] Cache'i V1 kapsamının dışında bırak.
- [x] Üç aşamalı sistem topolojisini kesinleştir: `Source → Transform → Sink`.
- [x] Timeout, late completion, duplicate work ve goodput semantiklerini kesinleştir.
- [x] Retry budget ile source load shedding'i ayrı deneyler hâline getir.
- [x] Aynı timestamp için event priority sırasını kesinleştir.
- [x] V1 kapsamını ve kapsam dışı özellikleri `system_architecture.md` içinde yaz.

## Bugün yapılacaklar

### 1. Araştırma protokolünü oluştur

- [x] `configs/research_protocol.yaml` dosyasını oluştur.
- [x] `model_version`, `protocol_version` ve `rng_version` alanlarını ekle.
- [x] Zaman birimini integer microsecond tick olarak belirt.
- [x] Source, Transform ve Sink kapasite/queue ayarlarını ekle.
- [x] Arrival ve service-demand dağılımlarını birimleriyle yaz.
- [x] Per-attempt timeout, overall deadline ve `max_attempts` değerlerini yaz.
- [x] Queue-full davranışını ve retryable cause listesini yaz.
- [x] Trigger başlangıcı, kapasite çarpanı ve süre listesini yaz.
- [x] `rho = [0.50, 0.65, 0.80, 0.90, 0.95]` matrisini yaz.
- [x] Policy listesini yaz: `immediate`, `exponential`, `full_jitter`, `retry_budget`.
- [x] Exponential base/cap, immediate epsilon ve retry-budget token/refill değerlerini dondur.
- [x] Source load shedding'i ayrı mitigation config'i olarak tanımla.
- [x] Ana seed listesini ve keyed RNG namespace'lerini yaz.
- [x] Warm-up, pre-trigger ve post-trigger zamanlarını yaz.
- [x] Recovery için `H=600 s`, üç adet 30 saniyelik pencere ve goodput/queue eşiklerini yaz.
- [x] `H=300/600/900 s` sensitivity değerlerini yaz.
- [x] Event cap ve simulator-error davranışını yaz.

### 2. Config şemasını oluştur

- [x] `schemas/run-config.schema.json` dosyasını oluştur.
- [x] Zorunlu alanları tanımla.
- [x] Kapasite, queue, timeout ve sürelerin pozitif olmasını zorunlu kıl.
- [x] `rho` değerlerini `0 < rho < 1` aralığıyla sınırla.
- [x] Trigger multiplier değerini `0 < multiplier <= 1` aralığıyla sınırla.
- [x] Policy adlarını enum ile sınırla.
- [ ] Bilinmeyen alanları veya eksik sürüm bilgisini reddet.
- [x] `trigger_start < trigger_end` ve horizon ilişkileri için uygulama-seviyesi validation notu ekle.

### 3. Geçerli ve geçersiz örnekler hazırla

- [x] `configs/smoke.yaml` dosyasını küçük ve hızlı çalışan geçerli config olarak doldur.
- [x] En az bir geçerli canonical config hazırla.
- [x] Negatif kapasite örneğinin reddedildiğini test et.
- [x] Sıfır/negatif timeout örneğinin reddedildiğini test et.
- [x] `rho >= 1` örneğinin reddedildiğini test et.
- [x] Bilinmeyen policy örneğinin reddedildiğini test et.
- [x] Çakışan veya geçersiz trigger zamanı örneğinin reddedildiğini test et.

### 4. Canonical protocol hash

- [x] YAML'ı deterministik/canonical bir yapıya dönüştür.
- [x] Canonical içerikten SHA-256 üret.
- [x] Aynı config'in iki çalıştırmada aynı hash'i verdiğini test et.
- [x] Anlamlı bir parametre değiştiğinde hash'in değiştiğini test et.
- [x] Üretilen hash'i koşu metadata'sına yazmak üzere alan tanımla.

### 5. Son tutarlılık kontrolü

- [x] `research_question.md`, `state_machine.md`, `system_architecture.md` ve YAML arasında çelişki olmadığını kontrol et.
- [x] `3000 run` ile `3000 request` kavramlarının hiçbir yerde karışmadığını kontrol et.
- [x] Retry budget kolunun source shedding yapmadığını kontrol et.
- [x] Late completion'ın goodput üretmediğini kontrol et.
- [x] Trigger sonunda queue ve inflight işlerin temizlenmediğini kontrol et.
- [x] Bütün sayısal sabitlerin birimi ve kaynağı/rolü yazılı olsun: `literature fixture`, `model assumption` veya `experimental factor`.

## Gün 2 kabul kriterleri

- [x] `configs/research_protocol.yaml` geçerli şema kontrolünden geçiyor.
- [ ] Geçersiz config örnekleri beklenen nedenle reddediliyor.
- [x] Aynı canonical config aynı SHA-256 değerini üretiyor.
- [x] “Bu olay gerçekleşirse simülatör ne yapar?” sorusunda muğlak state transition kalmadı.
- [x] Ana sonuç görülmeden önce protokol ve model sürümü kaydedildi.

## Kapanış kapısı

Semantik çelişki, belirsiz timeout/duplicate davranışı veya doğrulanamayan config kalmadı. Gün 3 başlayabilir; ancak tam resolved-run schema ve golden fixture tamamlanmadan Rust simülasyon modeline geçilmez.

## Gün sonunda anlatabilmeliyim

> Python neden protokolü çözüp hash üretiyor, Rust neden yalnız tam çözülmüş tek-run JSON config okuyor?

## Başvuru belgeleri

- `docs/system_architecture.md`
- `docs/state_machine.md`
- `docs/research_question.md`
- `docs/literature_notes.md`
- `../../RETRYCASCADE_20_GUNLUK_ROADMAP.md`
- `../../RETRYCASCADE_BILIMSEL_ISTATISTIK_PROTOKOLU.md`
