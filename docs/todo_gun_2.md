# Gün 2 TODO — Semantiği ve Protokolü Kilitleme

## Günün amacı

Kodlamaya başlamadan önce simülasyonun bütün davranışlarını ve deney parametrelerini kesinleştirmek. Sonuçlar görüldükten sonra yapılan her semantik değişiklik yeni `model_version` ve `protocol_hash` gerektirecek.

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
- [ ] Zorunlu alanları tanımla.
- [ ] Kapasite, queue, timeout ve sürelerin pozitif olmasını zorunlu kıl.
- [ ] `rho` değerlerini `0 < rho < 1` aralığıyla sınırla.
- [ ] Trigger multiplier değerini `0 < multiplier <= 1` aralığıyla sınırla.
- [x] Policy adlarını enum ile sınırla.
- [ ] Bilinmeyen alanları veya eksik sürüm bilgisini reddet.
- [ ] `trigger_start < trigger_end` ve horizon ilişkileri için uygulama-seviyesi validation notu ekle.

### 3. Geçerli ve geçersiz örnekler hazırla

- [x] `configs/smoke.yaml` dosyasını küçük ve hızlı çalışan geçerli config olarak doldur.
- [x] En az bir geçerli canonical config hazırla.
- [ ] Negatif kapasite örneğinin reddedildiğini test et.
- [ ] Sıfır/negatif timeout örneğinin reddedildiğini test et.
- [ ] `rho >= 1` örneğinin reddedildiğini test et.
- [ ] Bilinmeyen policy örneğinin reddedildiğini test et.
- [ ] Çakışan veya geçersiz trigger zamanı örneğinin reddedildiğini test et.

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

- [ ] `configs/research_protocol.yaml` geçerli şema kontrolünden geçiyor.
- [ ] Geçersiz config örnekleri beklenen nedenle reddediliyor.
- [ ] Aynı canonical config aynı SHA-256 değerini üretiyor.
- [ ] “Bu olay gerçekleşirse simülatör ne yapar?” sorusunda muğlak state transition kalmadı.
- [ ] Ana sonuç görülmeden önce protokol ve model sürümü kaydedildi.

## Kill gate

Semantik çelişki, belirsiz timeout/duplicate davranışı veya doğrulanamayan config kalırsa Gün 3 Rust model koduna geçilmez.

## Başvuru belgeleri

- `docs/system_architecture.md`
- `docs/state_machine.md`
- `docs/research_question.md`
- `docs/literature_notes.md`
- `../../RETRYCASCADE_20_GUNLUK_ROADMAP.md`
- `../../RETRYCASCADE_BILIMSEL_ISTATISTIK_PROTOKOLU.md`
