# RetryCascade Makale Okuma Dizini

Bu puanlar makalelerin genel bilimsel kalitesini değil, **RetryCascade'in mevcut simülasyon çekirdeğine, akademik boşluğuna veya gelecekteki test ürününe doğrudan yararlılığını** gösterir. Dosya sisteminde `/` ayraç olduğu için dosya adlarında `5/5` yerine `5-of-5` kullanılmıştır.

Değerlendirmede her makalenin tam metni üzerinden problem, yöntem/model, deney veya vaka çalışmaları, ana sonuçlar ve sınırlılıklar incelenmiştir.

| Makale | Puan | RetryCascade için ana değer | Neden daha yüksek/düşük değil? |
|---|---:|---|---|
| [Metastable Failures in Distributed Systems](Metastable_Failures_in_Distributed_Systems_4-of-5.pdf) | 4/5 | Stable–vulnerable–metastable ayrımı, trigger/sustaining-effect çerçevesi ve retry-storm örneğinin temel kaynağıdır. | Yedi sayfalık kurucu/vision çalışmasıdır; formal model veya geniş kontrollü değerlendirme sunmaz. |
| [Metastable Failures in the Wild](Metastable_Failures_in_the_Wild_5-of-5.pdf) | 5/5 | 11 kurumdan 22 gerçek olay, trigger/amplification sınıflandırması ve kontrollü reproduction ile V1 problem tanımının ana ampirik dayanağıdır. | RetryCascade'in iddia sınırlarını ve recovery yorumunu doğrudan belirler. |
| [Analyzing Metastable Failures](Analyzing_Metastable_Failures_5-of-5.pdf) | 5/5 | CTMC → DES → emulator → stress-test zinciri, model ile gerçek sistem arasındaki doğrulama yolunu ve en yakın rakip yaklaşımı gösterir. | Bizim simülasyon + held-out testbed planımıza en doğrudan yöntemsel karşılaştırmadır. |
| [Formal Analysis of Metastable Failures in Software Systems](Formal_Analysis_of_Metastable_Failures_in_Software_Systems_4-of-5.pdf) | 4/5 | DSL, DES semantiği, veriyle kalibre CTMC, escape probability ve recovery-time analiziyle generic boundary iddiasının neden tek başına yeni olmadığını gösterir. | Çok değerlidir fakat CTMC/eigenvalue teorisi V1'in uygulama rotasının dışındadır. |
| [RetryGuard: Preventing Self-Inflicted Retry Storms](RetryGuard_Preventing_Self-Inflicted_Retry_Storms_5-of-5.pdf) | 5/5 | AWS ve Istio testbedlerinde standard/adaptive retry baselines, retry-storm büyüklüğü, latency, kaynak ve maliyet karşılaştırması sunar. | Genel policy tournament/controller alanının dolu olduğunu ve ürünümüzde hangi baselines'ın gerektiğini doğrudan gösterir. |
| [Characterizing Metastable Faults and Failures](Characterizing_Metastable_Faults_and_Failures_4-of-5.pdf) | 4/5 | Metastability'yi yalnız overload belirtisi yerine destabilizing component cycles ve scheduling üzerinden nedensel olarak ele alır. | Akademik çerçeveyi büyütür; Nyx/formal MFT rotası mevcut dar V1'in dışında kalır. |
| [Cancellation in Systems](Cancellation_in_Systems_5-of-5.pdf) | 5/5 | 13 sistemde 62 feature request ve 156 bug üzerinden cancellation initiation, propagation ve fulfillment ayrımını ampirik olarak kurar. | Measured partial-cancellation + held-out validation uzantımızın en önemli bağımsız dayanağıdır. |
| [The Tail at Scale](The_Tail_at_Scale_3-of-5.pdf) | 3/5 | Tail latency, hedged/tied requests, duplicate work, cancellation ve utilization ilişkisini açıklar. | Temel ve güçlü bir çalışmadır ancak retry-driven metastable recovery veya policy boundary çalışması değildir. |
| [Push-Button Reliability Testing with Rainmaker](Push-Button_Reliability_Testing_with_Rainmaker_4-of-5.pdf) | 4/5 | REST katmanında otomatik fault injection, mevcut testlerden workload seçimi, oracles ve 11 uygulamada 73 bug ile ürün yönü için güçlü tasarım örneğidir. | Retry politikası seçimi veya recovery tahmini yapmaz; ürün/test harness mimarisi için yakındır. |
| [Reliable Actors with Retry Orchestration](Reliable_Actors_with_Retry_Orchestration_3-of-5.pdf) | 3/5 | Retry parentage, completed invocation'ın tekrar edilmemesi ve side-effect güvenliği için formal programlama modeli ve fault-injection değerlendirmesi sunar. | Stateful writes ve actor orchestration V1 read-like kapsamının dışındadır; gelecek write desteği için değerlidir. |

## Okuma sırası

Projeyi anlamak için önerilen sıra:

1. *Metastable Failures in Distributed Systems*
2. *Metastable Failures in the Wild*
3. *Analyzing Metastable Failures*
4. *RetryGuard*
5. *Cancellation in Systems*
6. *Formal Analysis of Metastable Failures in Software Systems*
7. *Characterizing Metastable Faults and Failures*
8. *Push-Button Reliability Testing with Rainmaker*
9. *The Tail at Scale*
10. *Reliable Actors with Retry Orchestration*

## Kaynak bağlantıları

- https://sigops.org/s/conferences/hotos/2021/papers/hotos21-s11-bronson.pdf
- https://www.usenix.org/system/files/osdi22-huang-lexiang.pdf
- https://sigops.org/s/conferences/hotos/2025/papers/hotos25-106.pdf
- https://arxiv.org/abs/2510.03551
- https://arxiv.org/abs/2511.23278
- https://arxiv.org/abs/2606.00942
- https://www.usenix.org/system/files/osdi22-sethi.pdf
- https://barroso.org/publications/TheTailAtScale.pdf
- https://www.usenix.org/conference/nsdi23/presentation/chen-yinfang
- https://arxiv.org/abs/2111.11562
