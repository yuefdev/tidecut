# Astral Lunar

Astral Lunar, Svelte 5 + TypeScript arayüzü ve Tauri 2 + Rust masaüstü katmanı kullanan, tahribatsız kurgu odaklı bir video editörüdür. Önizleme Canvas/Web Audio ile; teslim, proxy ve cache işlemleri FFmpeg ile gerçekleştirilir.

## Özellikler

- Çok katmanlı video/görsel/metin kompozisyonu ve çok kanallı ses miksi
- Video, görsel ve ses içe aktarma; medya inceleme, tekilleştirme ve eksik dosya denetimi
- Trim kolları, ripple edit, split, waveform, kanal mute/solo/lock, gain ve pan
- Font, ağırlık, hizalama ve arka plan stilli metin klipleri; giriş/çıkış geçişleri; konum, ölçek, dönüş, opaklık, ses ve pan keyframe'leri; lineer/hold/ease eğrileri ve `0.05×–16×` hız
- Atomik `.astral` proje kaydı, checksum doğrulaması, `.bak` geri dönüşü, otomatik kayıt ve crash recovery
- Eksik medyayı dosya adı/boyut/hash ile yeniden bağlama; belirsiz eşleşmeleri kullanıcıya bırakma
- 540p/720p fingerprint tabanlı proxy cache, cache hit, LRU temizleme ve yarım dosya temizliği
- Gerçek H.264/AAC export; ilerleme, FPS, hız, ETA, iptal, yapılandırılmış hata raporu ve güvenli kısmi çıktı
- Kesin teslim presetleri: 9:16 `1080×1920`, 1:1 `1080×1080`, 16:9 `1920×1080`

## Mimari

- `src/lib/editor/`: timeline işlem motoru, kare planı, Canvas compositor ve Web Audio mixer
- `src/lib/project/`: proje şeması, masaüstü istemcisi, otomatik kayıt, oturum ve medya araçları
- `src/lib/render/`: frontend render DTO'ları, Tauri istemcisi ve timeline → render grafiği adaptörü
- `src/lib/components/`: editör panelleri, timeline, export, proxy, recovery ve relink arayüzleri
- `src-tauri/src/project.rs`: atomik kayıt/yükleme, bütünlük, backup, recovery ve medya inceleme
- `src-tauri/src/render.rs`: FFmpeg filter graph, iş kuyruğu, progress/cancel/error ve proxy cache

## Gereksinimler

- Node.js 20+
- Rust stable ve Tauri 2'nin platform bağımlılıkları
- Windows için WebView2

FFmpeg'in sistemde kurulu olması zorunlu değildir. Export veya proxy penceresindeki **Kur** eylemi, FFmpeg çalışma zamanını uygulamanın kendi cache dizinine indirir; ardından H.264/AAC encoder'ları ile compositor/mixer filtrelerinin varlığı doğrulanır. Sistem `PATH`'indeki uyumlu FFmpeg de otomatik algılanır.

## Geliştirme

```powershell
npm install
npm run dev
```

AI veya başka bir süreç Rust kodunu arka planda düzenlerken açık uygulamanın
yeniden başlamamasını istiyorsanız sabit geliştirme modunu kullanın:

```powershell
npm run dev:stable
```

Bu modda frontend Vite üzerinden güncellenmeye devam eder; Rust değişiklikleri
uygulamayı yeniden başlattığınızda devreye girer. Normal `npm run dev` modunda
ise Rust yeniden başlatmalarından sonra aktif çalışma otomatik geri yüklenir.

Sadece web arayüzünü çalıştırmak için:

```powershell
npm run vite:dev
```

Web önizlemesinde masaüstü dosya sistemi ve FFmpeg komutları bilinçli olarak devre dışıdır; gerçek proje ve render akışları `npm run dev` ile açılan Tauri penceresinde çalışır.

## Doğrulama

```powershell
npm run check
npm test
npm run build
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Proje ve güvenli kayıt davranışı

- Projeler sürümlü `.astral` zarfında saklanır; medya dosyaları projeye gömülmez, taşınabilir referanslar tutulur.
- Manuel kayıt temp dosya + flush/fsync + atomik replace uygular; önceki sağlam sürüm `.bak` olarak korunur.
- Otomatik kayıtlar uygulama veri dizinindeki `recovery` klasörüne proje başına tek snapshot yazar; yolu olmayan çalışma için de uygulamalar arasında korunan tek bir taslak snapshot kullanılır.
- Açılışta daha yeni bir recovery varsa geri yükleme/atlama seçimi sunulur.
- Bozuk veya checksum'u uyuşmayan ana dosyada sağlam backup otomatik olarak denenir ve durum kullanıcıya raporlanır.

## Desteklenen medya

İçe aktarma katmanı yaygın MP4/MOV/MKV/WebM/AVI/MXF/MTS/M2TS/TS/WMV/FLV/OGV videolarını; MP3/WAV/M4A/AAC/FLAC/OGG/Opus/AIFF/WMA seslerini; PNG/JPEG/WebP/GIF/BMP/TIFF/AVIF/HEIC/HEIF/SVG görsellerini tanır. WebView tarafından doğrudan çözülemeyen codec'ler için proxy üretimi önerilir.

## Export notları

- Teslim kapsayıcısı MP4, video codec'i H.264, ses codec'i AAC'dir.
- `contain` görüntüyü letterbox ile sığdırır; `cover` kadroyu doldurup taşanı kırpar.
- İptal veya hata durumunda hedef dosya korunur; yalnızca işe ait gizli kısmi dosya temizlenir.
- FFmpeg stderr kuyruğu teknik raporda tutulur, kullanıcıya ise kısa ve eyleme dönük hata gösterilir.
