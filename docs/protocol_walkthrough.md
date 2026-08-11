# Protokolü anlama notlarım

## sistem nasıl çalışıyor?
Source: attempt'in sisteme kabul edildiği giriş işlem aşaması
Transform: Ara katman 
Sink: Veritabanını temsil eden son ve en yavaş aşama.
Darboğaz: sistemde kapasitesi en düşük olan bileşen. bizde sink

source:140 
transform:120
sink:100

## Workload
rho: gelen istek hızının darboğaz kapasitesine oranı
6500 basis point: %65 demek
Poisson arrival: isteklerin düz bir sayı yerine Poisson dağılımı olarak gelmesi

## Retry 
Timeout ne zaman başlıyor: attempt source aşamasına kabul edildiğinde başlıyor.
Eski attemt'e ne oluyor: timeout olsa bile iptal edilmiyor sonradan bitse bile goodput sayılmıyor.
En fazla kaç attempt var: tolam 3: ilk attmpt + en fazla 2 retry

## Politikalar 
Immediate: Timeout sonrasında 1 mikrosaniye bekleyip hemen retry oluşturur.
Exponential: er retry'da bekleme süresini arttırır 25ms ->50 ms en fazla 200ms
full jitter: expeonential üst sınırına kadar rastgele bekler retryların aynı anda yığılmasını azaltmayı amaçlar
retry budget: retry hakkının token ile sınırlandırılması

## Trigger: 
Neyi değiştiriyor:  yalnız sink kapasitesini 100'den 50'ye düşürüyor.
Ne kadar sürüyor: deneye göre 1 3 10 veya 30 saniye
Bittiğinde hangi state temizlenmiyor: queue, çalışan attempt'ler ve planlanmış retry birikimi temizlenmiyor. yalnız sink kapasitesi tekrar 100'e dönüyor

## Recovery
Sistem ne zaman recovered sayılıyor: 3 kere 30 saniyenin hepsinde düzgün olması goodput baselineın en az %95'ine dönmüş olmalı
600 saniyede toparlanmazsa:  pre trigger ve no trigger koşulları kararlıysa ve sink kapasitesi tamamen geri geldiyse, koşu metastable faiulure olarak sınıflandırılır. 

