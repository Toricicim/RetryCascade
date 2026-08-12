# Gün 1 — Problem, Literatür ve Proje İskeleti

## Amaç

Araştırılan mekanizmayı, iddia sınırını ve temel kavramları anlamak; Rust/Python test iskeletini çalıştırmak.

## Must

- [x] HotOS ve OSDI kaynaklarını okumaya başla.
- [x] Stable, vulnerable ve operational metastable ayrımını yaz.
- [x] Logical request, attempt, throughput, goodput, trigger ve sustaining effect kavramlarını tanımla.
- [x] Araştırma sorusu ve iddia edilmeyecekleri yaz.
- [x] State-machine diyagramını oluştur.
- [x] Rust workspace ve Python paketini oluştur.
- [x] `cargo test --workspace` ve Python smoke testini çalıştır.

## Stretch

- [x] Table 1’deki trigger, sustaining effect ve mitigation örneklerini not al.

## Teslim

- `docs/research_question.md`
- `docs/state_machine.md`
- `docs/literature_notes.md`
- Çalışan Rust/Python test iskeleti

## Kabul

- [x] Bir logical request’in neden birden fazla attempt üretebildiğini açıklayabiliyorum.
- [x] Goodput ile fiziksel completion sayısının neden farklı olduğunu açıklayabiliyorum.
- [x] Simülasyonun production prediction olmadığını açıklayabiliyorum.

## Stop gate

- [x] Temel kavramlarda birbiriyle çelişen tanım kalmadı.

## Gün sonunda anlatabilmeliyim

> Geçici bir problem bittikten sonra retry geri beslemesi sistemi neden kötü durumda tutabilir?

