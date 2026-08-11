# RetryCascade Literatür Notları

Bu dosya okunan kaynaklardan çıkarılan kişisel notları içerir. Bir bölüm yalnız ilgili kaynak okunduktan sonra doldurulmalıdır.

## 1. Metastable Failures in Distributed Systems — HotOS 2021

**Kaynak:** Bronson, N., Aghayev, A., Charapko, A. ve Zhu, T. (2021). *Metastable Failures in Distributed Systems*.  
**Bağlantı:** https://doi.org/10.1145/3458336.3465286

### Ana fikirler

- Metastable failure, geçici trigger ortadan kalktıktan sonra sustaining effect nedeniyle kötü sistem durumunun devam etmesidir.
- Stable sistem tanımlanan trigger sonrasında kendiliğinden toparlanabilir; vulnerable sistem normal görünmesine rağmen aynı trigger sonrasında metastable duruma geçebilir.
- Trigger problemi başlatır, sustaining effect ise problemi devam ettirir.
- Retry, bir logical request için birden fazla fiziksel attempt oluşturarak work amplification üretebilir.
- Cache kaybı, pahalı hata işleme yolu ve connection-pool politikaları da geri besleme döngüsü oluşturabilir.
- Makale gerçek sistemlerdeki mekanizmayı destekler; RetryCascade simülatörünün sayısal parametrelerini doğrulamaz.

### RetryCascade açısından anlamı

- Logical request ile attempt ayrı kimliklerle izlenmelidir.
- Trigger sona erdikten sonraki davranış ayrıca ölçülmelidir.
- Goodput, toplam tamamlanan fiziksel işten ayrı tutulmalıdır.
- Proje gerçek üretim sistemini tahmin eden bir model olarak sunulmamalıdır.

## 2. Metastable Failures in the Wild — OSDI 2022

**Kaynak:** Huang, L. ve diğerleri (2022). *Metastable Failures in the Wild*.  
**Bağlantı:** https://www.usenix.org/conference/osdi22/presentation/huang-lexiang

**Okuma durumu:** Okunuyor.

### Okurken cevaplanacak sorular

1. Çalışmada kaç gerçek olay ve kaç kurum incelenmiş?
2. Gerçek olaylarda en sık görülen trigger türleri neler?
3. Sustaining effect hangi kaynakların tükenmesiyle oluşuyor?
4. Retry veya work amplification içeren hangi vakalar var?
5. Sistemler neden trigger ortadan kalkınca kendiliğinden toparlanamıyor?
6. Operatörler sistemi kurtarmak için hangi müdahaleleri uygulamış?
7. Çalışmanın gözlemsel verileri hangi sınırlılıklara sahip?
8. Bu çalışma RetryCascade modelindeki hangi varsayımları destekliyor?
9. Bu çalışma simülasyonumuzun hangi sayısal parametrelerini desteklemiyor?

### Okuma sonrası kısa özet

<!-- Kaynak okunduktan sonra 5–10 cümleyle doldur. -->

### RetryCascade için alınacak kararlar

<!-- Model veya rapor açısından çıkarılan kararları yaz. Sonuçları görmeden parametre değiştirme. -->

### Anlamadığım veya tekrar bakacağım noktalar

<!-- Bilinmeyen noktaları silmek yerine buraya kaydet. -->

