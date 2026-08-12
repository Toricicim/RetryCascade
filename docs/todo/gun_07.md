# Gün 7 TODO — Timeout, Deadline ve Lifecycle Ledger

## Amaç

Mantıksal kullanıcı isteği ile onun fiziksel attempt'lerini ayrı ve eksiksiz muhasebeleştirmek.

## Önkoşul

Trigger ve queue rejection testleri geçmeli.

## Must

- [ ] `LogicalRequest` ve `Attempt` state'lerini uygula.
- [ ] Source admission'da attempt timeout event'i planla.
- [ ] External arrival'da overall logical deadline event'i planla.
- [ ] Timeout olan attempt'i fiziksel olarak çalıştırmaya devam ettir.
- [ ] Deadline sonrası logical isteği terminal yap; inflight work'ü wasted/late olarak izle.
- [ ] İlk zamanında başarıyı unique goodput say; late ve duplicate'i goodput sayma.
- [ ] Horizon sonunda outstanding state'leri ledger'a kapat.
- [ ] Her logical request ve attempt için conservation invariant yaz.

## Stretch

- [ ] Lifecycle geçişlerini debug trace'te okunur göster.

## Teslim

Timeout/deadline ve first-success semantiği çalışan lifecycle ledger.

## Kabul testleri

- [ ] Completion ve timeout aynı tick'te ise completion kazanır.
- [ ] Deadline ve RetryReady aynı tick'te ise deadline kazanır.
- [ ] Timeout olmuş iş daha sonra kapasite tüketip late tamamlanır.
- [ ] İlk success'ten sonraki completion duplicate/wasted olur.
- [ ] Source rejection ve horizon senaryolarında ledger farkı sıfırdır.

## Stop gate

Her logical/attempt tam bir terminal muhasebe sınıfına girmiyorsa retry policy ekleme.

## Gün sonunda anlatabilmeliyim

Bir logical request neden birden fazla attempt üretir; timeout neden server işini otomatik iptal etmez?

## Sonraki güne devir

Trace güzelleştirme devredilebilir; ledger invariants devredilemez.
