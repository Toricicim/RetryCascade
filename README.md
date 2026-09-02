# RetryCascade

RetryCascade, geçici bir Sink kapasite kaybından sonra retry davranışının
kuyrukları, kullanıcıya yararlı işi (`goodput`) ve sistemin toparlanmasını nasıl
etkilediğini inceleyen yeniden üretilebilir bir ayrık olay simülasyonudur.

Bu ilk çekirdek bir production predictor değildir. V1 modeli bilinçli olarak
read-like ve idempotent logical request'ler, `Source -> Transform -> Sink`
pipeline'ı, bounded FIFO queue'lar, Sink-only trigger ve dört retry policy ile
sınırlıdır.

## Mimari

1. Python, araştırma protokolünü ve tek-run seçimini doğrular.
2. Python bütün varsayımları açık olan canonical bir
   `resolved_run_config.json` üretir ve SHA-256 provenance ekler.
3. Rust config'i tekrar strict biçimde doğrular ve deterministik simülasyonu
   yürütür.
4. CLI; sayaçları, retry amplification'ı, exact time-weighted queue metriğini,
   recovery pencerelerini, trace hash'lerini ve kapanan lifecycle ledger'ını
   sürümlü `run_summary.json` içinde üretir.

Desteklenen politikalar `immediate`, `exponential`, `full_jitter` ve
`retry_budget`'tır. Full-jitter ile retry-budget aynı keyed jitter çekilişini
kullanır; böylece budget karşılaştırmasında rastgelelik gereksiz bir fark
oluşturmaz.

## Çalıştırma

Önce committed küçük smoke config ile simülasyonu çalıştır:

```bash
cargo run --locked -p retrycascade-cli -- \
  tests/golden/resolved_smoke_config.json \
  run_summary.json
```

İkinci dosya yolu verilmezse JSON stdout'a yazılır:

```bash
cargo run --locked -p retrycascade-cli -- \
  tests/golden/resolved_smoke_config.json
```

Protokol veya seçim değiştiğinde golden fixture'ları Python otoritesinden tekrar
üret:

```bash
cd python
.venv/bin/python -m retrycascade.generate_fixtures
```

## Doğrulama

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cd python
.venv/bin/python -m pytest
```

Testler implementasyonun tanımlı sözleşmelere uyduğunu gösterir; modelin gerçek
bir production sistemini doğru tahmin ettiğini göstermez. Akademik sonuç için
bir sonraki aşamada faktör matrisi çoklu seed ile koşturulmalı, duyarlılık
analizi yapılmalı ve mümkünse gerçek bir sistemden alınan gözlemlerle mekanizma
doğrulanmalıdır.
