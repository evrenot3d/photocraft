# PhotoCraft — EVREN geliştirmesi (rehber)

Bu klasör, açık kaynak **PhotoCraft**'ın (https://github.com/storytold/photocraft, ArtCraft ailesi,
Rust + egui) bir kopyasıdır (fork): **PhotoCraft Evren**, GitHub `evrenot3d/photocraft`. Kendi özelliklerimiz buraya eklenir.

## Git düzeni

| Ad | Ne |
|---|---|
| `upstream` | Resmi PhotoCraft deposu (yalnız okunur, buradan güncelleme alınır) |
| `evren` (dal) | Bizim dal: resmi sürüm + EVREN eklentileri. Her zaman bu dalda çalışılır |
| `main` (dal) | Resmi `main`'in yerel kopyası (dokunulmaz) |

Başlangıç: resmi **v0.20.1** etiketi (bilgisayardaki kurulu PhotoCraft ile aynı sürüm).

## Güncelleme almak (yeni PhotoCraft sürümü çıktığında)

```powershell
cd "<proje klasörü>"   # bu bilgisayarda: Masaüstü\EVREN PROGRAMLAR\photocraft
.\EVREN_guncelle.ps1            # en yeni resmi sürüm etiketini birleştirir ve derler
.\EVREN_guncelle.ps1 -Surum v0.21.0   # belirli bir sürüm
```

Betik: değişiklikleri commit'lenmemişse durur → `git fetch upstream --tags` → sürüm etiketini
`evren` dalına birleştirir (merge) → çakışma varsa durur ve dosyaları listeler → `cargo build --release`.

## Eklentileri güncellemeye dayanıklı tutma kuralları

1. Kendi kodumuz **ayrı dosyalarda**:
   - Motor komutları: `crates/engine/src/cmd/evren_*_cmds.rs`
   - Arayüz: `crates/ui-egui/src/evren/`
2. Photocraft'ın kendi dosyalarına yalnız **tek satırlık bağlantılar** eklenir ve hepsi
   `// EVREN eklentisi` yorumuyla işaretlenir. Hepsini bulmak: `git grep -n "EVREN eklentisi"`.
3. Belge modeline (crates/doc) alan **eklenmez**. Ek veri katmanın `psd_blocks` listesinde kendi
   4 harfli anahtarlarımızla saklanır (Photocraft bunları .pcraft / PSD / TIFF'te aynen saklar).
4. Komut kimlikleri `evren.` ile başlar (resmi komutlarla karışmaz).

### Bağlantı satırları (şu an)

| Dosya | Ne için |
|---|---|
| `crates/engine/src/cmd.rs` | `pub mod evren_pano_cmds;` + `v.extend(crate::evren_pano_cmds::specs());` |
| `crates/engine/src/lib.rs` | `pub use cmd::evren_pano_cmds;` |
| `crates/ui-egui/src/lib.rs` | `mod evren;` |
| `crates/ui-egui/src/panels/layer_menu_ui.rs` | Katman sağ tık menüsüne 360 satırları |
| `crates/ui-egui/src/canvas/tool_input.rs` | Taşıma aracıyla 360 çevirme; Çizgi aracı (`evren::cizgi_ui::pointer`, Fırça seçiliyken) |
| `crates/ui-egui/src/app/frame.rs` | EVREN pencereleri (`evren::pencereler`: 360 Ayarları + yapay zekâ araçları) |
| `crates/engine/src/jobs.rs` (`after_command` başı) | Filtre/ayarlama/birleştirme sonrası 360 küresine işleme |
| `crates/ui-egui/src/panels/layer_row.rs` | Katman küçük resmine turuncu "360" rozeti |
| `crates/ui-egui/src/panels/toolbar.rs` | Sol menünün en altında EVREN düğmeleri (`evren::arac_cubugu`, + araç listesinden yer ayırma) |
| `crates/engine/src/cmd.rs` / `lib.rs` | `evren_ai_cmds` modülü ve `specs()` (yapay zekâ araçlarının belge komutları) |
| `crates/ui-egui/src/panels/panel_docking.rs` (`show`, DockArea öncesi) | Panel alanının zemini boyanır (siyah şerit düzeltmesi) |
| `crates/ui-egui/src/menus/invoke.rs` (`invoke_early_commands` başı) | Kayıt yeri soran EVREN komutları (360 Export) |
| `crates/ui-egui/src/canvas/wheel_nav.rs` (`read_with`, 2 satır) | Tekerlekle yakınlaştırma açıkken Ctrl / Alt + tekerlek de yakınlaştırır (kaydırmaz) |
| `crates/ui-egui/src/file_open.rs` (`open_file` ve masaüstü `open_path` başı) | .pdf → `evren::pdf_ui::yakala` (PDF İçe Aktar penceresi) |
| `apps/photocraft/src/services.rs` (`OPEN_EXTS`, `OPEN_FILTERS`) | File › Open listesinde PDF |

## Derleme / çalıştırma

- Rust: rustup (stable, MSVC). Visual Studio 2019 Build Tools kurulu.
- Derle: `cargo build --release -p photocraft` (ilk derleme uzun; sonrakiler yalnız değişen kısımları derler).
- Çalıştır: `EVREN_baslat.bat` (kurulu resmi PhotoCraft açıksa ona dosya göndermesin diye `--new-instance`).
- Program: `target\release\photocraft.exe`.

## Eklenen özellikler

### 1) 360 derece katmanı (2026-11-03)

- **Katmanlar paneli › sağ tık › "360 Derece Yap"** (piksel katmanı): katmanın düz panoraması
  (2:1 equirectangular, ör. DJI 360) katmanın içine JPEG (kalite 95) olarak saklanır, katman belge
  boyutunda perspektif görünüm olur. 360 katmanında sağ tık: **"360 Ayarları…"**, **"360'ı Kapat"**
  (düz panorama eski yerine geri gelir).
- **Taşıma aracı** + etkin katman 360: tuvalde sürüklemek görünümü çevirir (görüntü imleçle kayar,
  ). Sürükleme sırasında hızlı çizim, bırakınca tam kalite; bir sürükleme = tek geri alma adımı.
- **360 Ayarları penceresi**: Görüş açısı (FOV 10–150°, yatay), Yatay (±180°), Dikey (±90°),
  Eğim (±45°), 35 mm odak, Sıfırla, 360'ı Kapat.
- Saklama: katman blokları `evPs` (panorama JPEG) ve `evPv` (görünüm JSON: yaw, pitch, roll, fov,
  x, y). Dosyaya kaydedilip açılınca 360 katmanı olarak devam eder; katman çoğaltılınca kopya da 360.
- DERS: PSD'den okununca görünüm bloğunun sonunda sıfır dolgu baytları geliyor (4'ün katına
  doldurma); JSON okunmadan önce sondaki sıfırlar atılıyor (`view_of`). Test: .pcraft ve .psd'ye
  kaydet → aç → 360 açıları aynı.
- Test (2026-11-03): 4096×2048 test panoraması; 360 yap 0,17 sn, çevirme 0,03 sn; sürükleme tek
  geri alma adımı; sağ tık menüsü, 360 Ayarları penceresi, Sıfırla, 360'ı Kapat denendi.
- (Madde 2 ile değişti: birleştirme ve doğrudan çizim artık küreye işleniyor.)
- Matematik: R = Ry(yaw)·Rx(pitch)·Rz(roll), yön → boylam/enlem.
- Kod: `crates/engine/src/cmd/evren_pano_cmds.rs` (komutlar `evren.pano.make / view / info / off`),
  `crates/ui-egui/src/evren/pano_ui.rs`.
- Yapılacaklar (sonra): ok tuşlarıyla çevirme, Alt+tekerlek FOV, GPU ile çizim.

### 2) 360: filtre/ayarlama kürenin tamamına, küreye birleştirme, pin, rozet, sol düğme (2026-11-04)

- **Filtre ve ayarlamalar** (Filtre menüsü, Camera Raw, Görüntü › Ayarlamalar; komut kimliği
  `filter.*` / `image.adjustments.*`): 360 katmanına uygulanınca motorun `after_command`'ı
  (`evren_pano_cmds::after_command`) aynı komutu aynı ayarlarla **görünmez ayrı bir oturumda kürenin
  tamamına** uygular (`replay_on_sphere`), görünüm yeni küreden yeniden çizilir. Komutun kendi geri
  alma adımına katılır (`edit_with(.., false, ..)`), Ctrl+Z eski küreyi geri getirir. Seçim varken
  küreye tekrar uygulanmaz (yalnız seçili yer değişir, çevirince aşağıdaki "çizim" yolu küreye işler).
  Önceki hâl: `history.recorded_before()`. Test: Camera Raw doygunluk -100 → yaw 150'de arka taraf
  da gri; Ters çevir → yaw -60'ta da ters; geri alma çalışıyor.
- **Küreye birleştirme**: `layer.merge*` (Aşağı Birleştir, Seçilileri Birleştir) sonucu 360 katmanının
  kimliğindeyse 360 bilgisi geri konur, üstten gelenler küreye işlenir (`bake`: görünümde kürenin
  o açıdaki çiziminden farklı pikseller → her küre pikseli için yön → kamera → görünümdeyse karıştır),
  katman 360 kalır ve döndürülebilir. Test: pin aşağı birleştirildi, yaw 25 / pitch 10'da küreyle döndü.
- **Doğrudan çizim** (fırça, dolgu, yapıştırma … 360 katmanının içine): çevirmeden önce `view_cmd`
  aynı `bake` ile küreye işler (bir sürüklemede yalnız ilk adımda denetlenir, `LAST_CHECK`).
- **Pin** (`evren.pano.pin {x,y,size?}`): 360 Ayarları penceresinde turuncu Google Haritalar biçimli
  işaret; sürükleyip tuvale bırakınca ucu bırakılan noktada en üste yeni "Pin" katmanı (boy belge
  yüksekliğinin %9'u), tıklayınca ortaya. Görsel `pin_image` (4×4 örnekli kenar yumuşatma).
- **Rozet**: Katmanlar panelinde 360 katmanının küçük resminin sağ altında turuncu "360".
- **Sol menü**: araç çubuğunun en altında (renk kareleri ve ekran modu düğmesinin altında, ince çizgiyle
  ayrılmış) turuncu "360" düğmesi → 360 Ayarları penceresini açar/kapatır. EVREN eklentilerinin
  düğmeleri buraya eklenecek (`pano_ui::toolbar_buttons`, yüksekliği `toolbar_height`).
- 360 Ayarları penceresinin hedefi: etkin katman 360 ise o, değilse belgedeki ilk 360 katmanı.
  Bölümler: Görünüm (kaydırıcılar, Sıfırla), Pin, Katman (360'ı Kapat).

### 3) 360 Export (2026-11-04)

- **Dosya › Dışa Aktar › 360 Export…** (komut `evren.pano.export {path, layer?, quality?=95}`; menü
  yeri komutun `menu: &["File","Export"]` alanından kendiliğinden gelir) ve 360 Ayarları penceresinde
  Katman › "360 Export…". Kayıt yeri Windows'un kayıt penceresiyle sorulur (`pano_ui::invoke` →
  `app.pick_save`, `menus/invoke.rs` `invoke_early_commands` başındaki bağlantı satırı); önerilen ad
  `<belge>_360.jpg`.
- Çıktı: kürenin tamamı, düz 2:1 panorama, tam çözünürlük (DJI Mini 4 Pro'nun 360 JPG'si gibi).
  Ekranda görünen üst katmanlar (pin, yazı …) ve 360 katmanına işlenmemiş düzenlemeler küreye işlenir
  (`bake`, belge tam görünümü `pixels::composite_layers` ile); belge değişmez, katmanlar durur.
- Hedef: etkin katman 360 ise o, değilse belgedeki ilk 360 katmanı.
- Biçim uzantıdan: .jpg (kalite 95, 4:4:4) — fotoğrafın kendi EXIF'i (kamera, GPS) ve XMP'si (GPano,
  drone-dji) korunur; XMP'de GPano yoksa standart GPano eklenir (Google Foto / Facebook / Windows küre
  olarak açar). .png / .tif — yalnız pikseller (GPano yok).
- Fotoğraf bilgisi: "360 Derece Yap" sırasında belge bu panoramanın kendisiyse (boyut aynı) ya da XMP'si
  GPano içeriyorsa EXIF + XMP katmanda `evPm` bloğuna saklanır (`[EXIF uzunluğu u32 BE][EXIF][XMP]`);
  eski 360 katmanlarında yoksa belgenin kendi bilgisi kullanılır.
- Test: DJI bilgili test JPG (Make DJI, Model FC8482, GPS, GPano + drone-dji XMP) → 360 yap, yaw 90,
  pin → 360 Export: 4096×2048, 0,26 sn, EXIF/GPS/GPano/drone-dji korunmuş, pin kürede SAĞ 90
  yönünde (x≈3072). Menüden tıklayınca "Farklı Kaydet" penceresi açılıyor.

### 4) Yapay zekâ araçları: Upscale, Alan derinliği, Bitki / Cam ayır, Gökyüzü + yeni simgeler (2026-11-04)

- **Sol menünün en altı** (ince çizginin altında, `evren/arac_cubugu.rs`): 360 · Upscale · Alan derinliği ·
  Bitkileri ayır · Camları ayır · Gökyüzü değiştir. Simgeler kendi SVG'lerimiz (`crates/ui-egui/src/evren/icons/`,
  özgün çizim), PhotoCraft'ın simgeleri gibi beyaz çizilip tema rengine boyanır (`craft_ui::icons::SvgIconSet`).
  Araç listesi bu düğmeler kadar kısalır (`yukseklik`), düğmeler hep görünür.
- **evren_ai klasörü** (depo kökünde; büyük kısımlar `evren_ai/.gitignore` ile git dışında):
  `sunucu.py` (iş sunucusu), `motor.py` (ComfyUI, port 8195), `gorus.py` (SAM 2.1,
  SegFormer, Grounding DINO, Depth Anything V2), `efekt.py` (alan derinliği + gökyüzü, OpenCV),
  `.venv` (Python 3.11: torch 2.11 cu128, transformers 4.57; install.ps1 kurar), `ComfyUI` (kod +
  yalnız ComfyUI-SUPIR eklentisi), `ComfyUI/models` (SUPIR-v0Q_fp16, Juggernaut XL v9, 4x-UltraSharp,
  hf/ dört algılama modeli), `gokyuzu_kutuphanesi`, `calisma` (geçici). Toplam ~19 GB.
- Modeller `evren_ai\ComfyUI\models` altında; başka bilgisayarda `evren_ai\install.ps1` kurar (MODELS.md).
- **Süreç yalnız iş sırasında açık** (`evren/ai_ui.rs`): PhotoCraft `evren_ai\.venv\Scripts\python.exe -u
  sunucu.py`'yi pencere açmadan başlatır, satır satır JSON konuşur (stdin istek, stdout cevap; günlük
  `evren_ai\sunucu.log`, ComfyUI günlüğü `evren_ai\motor.log`). Upscale / Bitki / Cam işi bitince, Alan
  derinliği / Gökyüzü penceresi kapanınca girdi kapatılır → sunucu ComfyUI'yi kapatıp çıkar (20 sn'de
  çıkmazsa ağaç öldürülür). İptal: `taskkill /T /F` (ComfyUI dahil). ComfyUI yalnız Upscale'de açılır.
- Belge tarafı komutlar (`engine/src/cmd/evren_ai_cmds.rs`): `evren.ai.composite {path, white}` (görünen
  hâl PNG), `evren.ai.addLayer {path, name}` (en üste katman), `evren.ai.upscaleApply {path, name}`
  (`image.imageSize` Lanczos ile belge sonucun boyutuna + sonuç katmanı, tek geri alma adımı, coalesce).
- **Upscale penceresi**: Model SUPIR (detay ekler, adım 15/20/30/45, varsayılan 30) / UltraSharp (hızlı);
  Büyütme 1 · 1,5 · 2 · 4 · 8x + özel (1–8); hedef 8'in katı, en çok 16000 px kenar /
  250 MP. "Tümüne uygula…" → açık sekmeler onay kutularıyla, "Seçilenlere uygula (n)": sekmeler sırayla
  (ComfyUI bir kez açılır), her sekmede tuval + katmanlar büyür, sonuç en üste "Upscale · Model Nx".
  İlerleme penceresi: Sekme i/n, aşama, yüzde, İptal.
- **Alan derinliği**: Depth Anything haritası bir kez hesaplanır (süreç açıkken bellekte), önizleme ≤900 px;
  önizlemeye tık = odak; Bulanıklık, Net kalan derinlik, Önü ve arkası / Yalnız arkası / Yalnız önü.
  Uygula → tam çözünürlük, yeni katman "Alan derinliği".
- **Gökyüzü değiştir**: kütüphane küçük resimleri (`_k`), Gökyüzü ekle… (dosya seç → kütüphaneye kopya),
  Klasörü aç; Kenarı kaydır, Yumuşaklık, Ölçek, Yatay/Dikey konum, Parlaklık, Sıcaklık, Ön plan uyumu,
  Kenar ışığı (çift tık varsayılan), Ayarları sıfırla; seçim ve ayarlar oturum boyunca hatırlanır.
  Uygula → yeni katman "Gökyüzü".
- **Bitkileri / Camları ayır**: pencere yok, ilerleme kutusu; sonuç saydam yeni katman "Bitkiler (ayrıldı)" /
  "Camlar (ayrıldı)" (SegFormer + Grounding DINO + SAM).
- Test (2026-11-04, 2500×1406 mimari render): bitki ~25 sn, cam ~30 sn, gökyüzü önizleme + uygula,
  alan derinliği (odak çimlerde, bina bulanık), Upscale UltraSharp 2x üç sekmeye birden ~70 sn (5000×2816,
  1600×904, 2400×1600), SUPIR 800→1600 px 15 adım ~95 sn. Her iş sonunda python / ComfyUI süreci kapandı.

### 5) Siyah şerit düzeltmesi + sol alt düğmeler çift sütun ve sürükle-bırak (2026-11-04)

- SİYAH ŞERİT: tuval paneli ile sağdaki dar (simge) panel sütunu arasında 7 piksellik boşluk hiçbir panel
  tarafından boyanmıyordu, pencerenin siyah zemini (#080808) görünüyordu (resmi PhotoCraft 0.20.1'de de var;
  craft-libs yerleşimi). Düzeltme: `panel_docking::show` panel alanını DockArea çizilmeden önce `t.chrome`
  ile boyar; boşluk artık panel grisi. Test: x 1982–1988'deki pikseller (8,8,8) → (83,83,83).
- SOL ALT DÜĞMELER (`evren/arac_cubugu.rs`): araç çubuğu çift sütunsa (`app.ui.panels.toolbar_double`)
  düğmeler de iki sütun (araç ızgarasıyla hizalı; yükseklik `yukseklik(kutu, cift)`).
- Sürükle-bırak sıralama: düğmede basılı tutup 6 px'ten fazla kaydırınca sürükleme başlar (egui'nin kendi
  sürüklemesini araç çubuğu üstlendiği için elle izleniyor), simge işaretçiyle gider, eski yeri çerçeveli
  boş kalır, bırakılan yere en yakın yuvaya taşınır. Sıra `evren_aiyarlar.json` › `dugme_sirasi`
  (gitignore). Yeni düğme eklenince listede yoksa sona eklenir. Tıklama eskisi gibi aracı açar/kapatır.

### 6) Proje klasörü taşındı (2026-11-04)

- `Masaüstü\PROGRAMLAR\photocraft` → `Masaüstü\EVREN PROGRAMLAR\photocraft`
  (kullanıcı isteği; diğer EVREN programlarının yanında). Git deposu, `evren_ai` (Python ortamı, ComfyUI,
  modeller) ve `target` birlikte taşındı. Masaüstü kısayolu "PhotoCraft Evren" yeni yola güncellendi.
- Kodda sabit yol yok: program `evren_ai` klasörünü kendi exe'sinin üst klasörlerinde arar
  (`ai_ui::kok`), betikler kendi klasörünü kullanır (`%~dp0`, `$PSScriptRoot`). Eski `PROGRAMLAR` klasörü boş kaldı.
- DERS: klasör taşınınca ilk `cargo build` PhotoCraft'ın derleme betikleri (photocraft-text yazı tipleri,
  ui-egui çeviri kataloğu) eski yolu aradığı için durdu. Çözüm: `target\release\build\photocraft-*` ve
  `target\release\.fingerprint\photocraft-*` silinip yeniden derlendi (dış kütüphaneler yeniden derlenmez, ~8 dk).

### 7) Fare tekerleği ile yakınlaştırma (2026-11-04)

- Kullanıcı isteği: tekerlek ileri = yakınlaştır, geri = uzaklaştır (Photoshop'taki "Zoom with Scroll Wheel").
- Kod değişikliği YOK: PhotoCraft'ın kendi ayarı açıldı. Edit › Preferences › General › Zoom with Scroll Wheel
  = `general.zoomWithScrollWheel: true`, dosya `%APPDATA%\Photocraft\preferences.json` (resmi PhotoCraft ile ortak;
  güncellemeler bu ayarı silmez). Eski hali yedek: `preferences.json.yedek-tekerlek`.
- Bu ayar açıkken: Alt + tekerlek = kaydırma, Shift + tekerlek = hazır yakınlık basamaklarına atlama.
  Her tık ×1,1 (`canvas\wheel_nav.rs`). Denendi: gerçek tekerlek 3 tık ileri %69 → %92, 5 tık geri → %57.
- Test notu: kullanıcının ayarlarıyla kontrol kanalı açınca `app.open` yalnız izin verilen klasörü açar;
  `--automation-read-root <klasör> --automation-write-root <klasör>` verilmeli.
- Ek (kullanıcı isteği): Ctrl ya da Alt basılıyken tekerlek kaydırmasın, yine yakınlaştırsın. `wheel_nav.rs`
  `read_with` içinde 2 işaretli satır: Zoom with Scroll Wheel açıkken Alt yok sayılır ve düz tekerlek süzgeci
  Ctrl / Alt / Cmd'li tekerlek olaylarını da alır (Shift + tekerlek = basamaklı yakınlaştırma aynen kalır).
  Gezgin ve Liquify penceresi de aynı davranır. Upstream'in `zoom_with_scroll_wheel_preference_swaps_wheel_and_alt_wheel`
  testi bu yüzden artık başarısız olur (bilerek). Denendi: tuşsuz / Ctrl / Alt + 2 tık ileri hepsi %69 → %83,5,
  2 tık geri %69; görünüm merkezi üçünde de aynı (yana kaydırma yok).

### 8) Gürültü temizleme (Denoise, ekran kartında) (2026-11-04)

- Kullanıcı isteği: Corona Renderer'daki denoiser gibi, açık kaynak, ekran kartıyla; düğmeye basınca ayarlar
  seçilip canlı uygulanır. Sol alttaki düğmelere "gurultu" eklendi (simge `evren/icons/gurultu.svg`).
- Temizleyici: **Intel Open Image Denoise 2.5.1** (https://github.com/RenderKit/oidn, Apache 2.0; Corona, V-Ray,
  Blender'ın da kullandığı yapay zekâ temizleyici). RTX 5060 (Blackwell) CUDA ile desteklenir (OIDN 2.4+).
  DLL'ler `evren_ai\oidn\bin` (git'e girmez; yalnız CPU + CUDA aygıtları, ~53 MB). Yeni sürüm: zip'in `bin`
  klasöründen aynı DLL'leri kopyala.
- PhotoCraft'ta `unsafe` yasak (çalışma alanı `unsafe_code = "forbid"`), OIDN C kütüphanesi: bu yüzden ayrı küçük
  yardımcı `evren_ai\oidn_yardimci` (kendi `[workspace]`'i olan ayrı Cargo projesi, bağımlılık yok, `raw-dylib`).
  Derleme: `evren_ai\oidn_yardimci\derle.ps1` → `evren_ai\oidn\bin\evren_oidn.exe`. Konuşma stdin/stdout ikili:
  `TEMIZLE w h kalite cihaz` + RGB8 → `OK ms cihaz adı` + RGB8 (ayrıntı main.rs başında). Yardımcı yalnız pencere
  açıkken çalışır; ekran kartı (CUDA) ve süzgeç pencere boyunca hazırda bekler, ayar değişince hızlıca yeniden temizler.
- Arayüz `evren/gurultu_ui.rs`. **Yeni katman AÇILMAZ** (kullanıcı isteği, 2026-11-04): pencere açılınca seçili
  katman temizlenir ve sonuç o katmanın kendi piksellerine yazılır (tuvalde canlı). Ayarlar: Cihaz (Otomatik /
  Ekran kartı / İşlemci), Kalite (Hızlı / Dengeli / Yüksek = OIDN quality 4/5/6), Miktar (özgün ile temiz sonucun
  karışımı, Corona'daki "Denoise amount"), Keskinlik (3×3 unsharp), Önizleme (kapalıyken özgün pikseller yazılır =
  önce/sonra). Miktar ve Keskinlik sürüklerken de canlı (en fazla 150 ms'de bir yazılır). Kalite/Cihaz değişince
  yeniden temizlenir. Ayarlar `evren_ai\ayarlar.json` → "gurultu".
- Belge tarafı `evren_ai_cmds.rs`: `katman_kaynak` katmanı KENDİ biçiminde (8/16/32 bit, `read_region`) dolu
  bölgesinden (`content_bounds`) okur; `evren.denoise.apply` aynı bölgeye `write_region_par` ile yazar (pikseller
  `piksel_birak` ile, JSON'a sığmaz). Saydamlık, maske, efektler, bölge dışı dokunulmaz. Yalnız RGB belge, kilitsiz
  resim katmanı (metin/şekil/akıllı nesne/grup için "önce rasterleştirin" uyarısı). Pencere boyunca hepsi aynı
  `coalesce` anahtarıyla TEK geri alma adımı. İptal: son adım bizimse `edit.undo`, değilse özgün pikseller geri
  yazılır. Uygula / pencereyi kapatma / düğmeye tekrar basma: temizlenmiş hâl kalır.
- Temizleyiciye 8 bit sRGB gönderilir (OIDN `srgb=true`, `hdr=false`); karışım katmanın kendi hassasiyetinde yapılır.

### 9) İnsanları düzelt (Detailer) (2026-11-04)

- Kullanıcı isteği: yapay zekâ / upscale sonrası mimari görsellerde uzaktaki insanların yüzleri ve vücutları bozuk.
  Sol alttaki düğmelere "insan" eklendi (simge `evren/icons/insan.svg`). Sonuç YENİ KATMAN: "İnsanlar (düzeltildi)",
  yalnız düzeltilen insanlar, gerisi saydam. Seçim varsa (ör. dikdörtgen) yalnız seçimin sınır kutusu işlenir ve
  yamalar seçimin dışına taşmaz; seçim yoksa bütün görsel.
- Yöntem: ComfyUI Impact Pack'teki FaceDetailer / ADetailer mantığı, Impact Pack KURULMADAN, zaten kurulu modellerle
  (`sunucu.py` → `is_insan`): Grounding DINO "person." (bölge ~640 px'lik örtüşen parçalara bölünür; uzaktaki küçük
  insanlar tek bakışta kaçar), iç içe kutular elenir; her insanın çevresi kesilir, SAM 2.1 sınırı çıkarır (büyütülmüş
  parçada), `gorus.bosalt()` ile VRAM boşaltılır; ComfyUI'de 4x-UltraSharp ile ~1024 px'e büyütülür, Juggernaut XL
  yalnız maskenin içini yeniden çizer (VAEEncode + SetLatentNoiseMask + KSampler dpmpp_2m karras, 30 adım, cfg 5,
  denoise `guc` = 0.5); sonuç küçültülüp yumuşak kenarla yerine konur. Boyu 640 px'ten uzun (yakındaki) insanlara
  dokunulmaz, 10 px'ten kısa olanlar atlanır, en çok 60 kişi.
- `gorus.py`: `kutu_bul` (DINO kutu + skor), `kutu_maskesi` (SAM, kutudan maske).
- PhotoCraft: `ai_ui.rs` `Arac::Insan` (Bitki/Cam ile aynı akış, `AyirDurum`'a `yuzde` ve `not` eklendi),
  `secim_kutusu` (doc.selection.content_bounds ∩ belge). Sonuç `evren.ai.addLayer` ile en üste.
- Denendi: 2500×1406 mimari render bütün görsel 10 kişi; PhotoCraft'ta 600×260 dikdörtgen seçimle 5 kişi ~2 dk
  (ComfyUI açılışı dahil), yamalar seçimin içinde, iş bitince Python ve ComfyUI kapandı.
- **İkinci sürüm (kullanıcı: "yüzler yine yamuk, tamamen yeniden ve düzgün oluşturmalı", güç en yüksek):**
  ilk sürüm denoise 0.5 ile eski çizimi koruyordu, yüze de ~100 px kalıyordu. Şimdi iki aşama, ayar penceresi yok,
  hep en güçlü ayar: (1) vücut denoise 0.75 + derinlik ControlNet'i (Depth Anything V2 haritası → xinsir
  `controlnet-union-sdxl-1.0-promax.safetensors`, `ComfyUI\models\controlnet`, 2.5 GB, Apache 2.0; ComfyUI'nin
  kendi `SetUnionControlNetType` "depth" + `ControlNetApplyAdvanced`, güç 0.85, bitiş 0.9) duruşu ve yeri korur;
  (2) DINO "face." ile görünen yüz (kişinin üst %45'i; arkası dönük insanda yüz çizilmez) 2.4× kare kesilip 1024 px'e
  büyütülür, yüz elipsi denoise 0.72 + yüz prompt'u + derinlik CN 0.5 ile sıfırdan çizilir, vücudun üstüne konur.
  ControlNet dosyası yoksa ControlNet'siz çalışır (durum satırı uyarır). Sabitler `sunucu.py`: GOVDE_GUC, YUZ_GUC,
  CN_DOSYA, YUZ_POZ/NEG. Denendi: aynı render, bütün görsel 10 kişi + 5 yüz ~6 dk (ComfyUI açılışı dahil); yüzler
  tamamen yeni ve düzgün, duruş korunuyor.

### 10) PDF açma (Photoshop'taki PDF İçe Aktar gibi) (2026-11-04)

- PhotoCraft PDF'i yalnız kaydediyordu. Artık File › Open (listede "PDF"), çift tıklama / komut satırı, Son Açılanlar
  ve pencereye sürükle-bırak .pdf'i "PDF İçe Aktar — dosya.pdf" penceresine yollar (`evren/pdf_ui.rs`): sayfa küçük
  resimleri (tık: tek sayfa, Ctrl/Shift+tık: çoklu, Tümünü seç / Hiçbiri), Çözünürlük (ppi; 150/300/600, varsayılan
  300), Görüntü boyutu (piksel + cm), Arka plan Saydam (Photoshop gibi) / Beyaz, Tamam → her sayfa ayrı belge
  ("ad" ya da "ad - Sayfa N", dosyasız: Kaydet yer sorar), belge ppi'si seçilen değer. Tuvale sürükle-bırak (yerleştir)
  bu yoldan geçmez.
- Çizici: **Windows'un kendi PDF motoru** (Windows.Data.Pdf, WinRT) — ek program / indirme yok. `evren_ai\pdf_ac.ps1`
  Windows PowerShell 5.1 ile (`powershell.exe`, pwsh 7 değil) çalışır: `-Bilgi` sayfa boyutları (pt) + küçük resimler,
  `-Sayfalar "1,3" -Dpi 300 [-Beyaz]` → s_<n>.png (ppi, System.Drawing ile PNG'ye yazılır). DERS: Windows çizici
  çıktıyı ekran ölçeğiyle büyütür (%175 ekranda 300 dpi yerine ~525): betik bir deneme çizimiyle oranı ölçüp düzeltir.
  Geçici dosyalar `evren_ai\calisma\pdf\<id>`, iş bitince / İptal'de silinir.
- Denendi: Masaüstü1.pdf (A3 plan) komut satırıyla açıldı → pencere, Tamam'a gerçek fare tıklaması → "plan"
  4961×3509, 300 ppi, saydam arka plan, tek katman; çizgiler ve yazılar net; geçici klasör silindi.

### 11) Çizgi aracı (Procreate QuickShape gibi) (2026-11-04)

- Sol alttaki "cizgi" düğmesi (simge `icons/cizgi.svg`) düğmenin yanında küçük menü açar: **Çizgi** / **Ok**
  (`ok.svg`), açıkken "Çizgiyi kapat". Seçilince PhotoCraft'ın **Fırça** aracı seçilir: kalınlık = fırça boyutu
  (sağ tık fırça penceresi, boyut halkası aynen), renk = ön plan rengi, opaklık = fırça opaklığı. Başka araç
  seçilince mod kapanır (`cizgi_ui::pencere` her karede bakar).
- `evren/cizgi_ui.rs`: `pointer` (tool_input.rs bağlantısı) Fırça + mod açıkken sol tık çizimini üstlenir (Alt =
  damlalık, Ctrl yine PhotoCraft'ın). Çizerken canlı önizleme (Foreground katmanında ekran çizimi). Fareyi BIRAKMADAN
  450 ms içinde 4 ekran pikselinden az kıpırdanırsa `tani()` çalışır: düz/eğik çizgi (sapma < %7; yataya/dikeye 3°
  yakınsa tam düz), kapalı yol → köşe sayımı (RDP %3, dönüş > 35°) ile çokgen ya da PCA elips (eksenler ~eşitse tam
  daire), 4 köşe ~90° → düzgün dikdörtgen (yataya 5° yakınsa hizalı); açık yol → önce yay (Kåsa çember uydurma,
  hata < %5), sonra köşeli çizgi (kaba ve ince RDP aynı köşeleri verir + dönüş > 40°), yoksa Catmull-Rom yumuşak
  eğri. Düzleşen düz çizginin ucu fareyle sürüklenebilir. Beklemeden bırakılırsa serbest çizim (hafif yumuşatılmış).
- Ok: yolun sonundaki yön, ok başı boyu max(4×kalınlık, 12), gövde ok başının ortasında biter; kapalı şekillerde ok yok.
- Katmana çizim: `rasterle()` yuvarlak uçlu kalın çizgi + ok üçgeni → pürüzsüz kenarlı kapsama haritası (parça
  uzaklığı, satırlar paralel), belge sınırında kırpılır; `evren.cizgi.ciz` (evren_ai_cmds.rs, `cizgi_birak`) ön plan
  rengiyle seçili katmana boyar (seçim varsa yalnız içine, saydamlık kilidinde alfa korunur). Seçili katman
  boyanamıyorsa (metin, grup, kilitli…) üstüne "Çizgi" katmanı açılır. Her çizim tek geri alma adımı ("Çizgi").
- Denendi (gerçek fare, ±3 px titreme, 0,9 sn bekleme): eğik çizgi, dalgalı yatay → tam yatay, daire, kare, üçgen,
  yay, L (keskin köşe), S (yumuşak), Ok modunda düz ok, eğri ok ve beklemeden serbest ok; hepsi pürüzsüz.
  İlk sürümde yay köşeli çiziliyordu → açık yolda yay sınaması öne alındı.

### 12) GitHub yayını: PhotoCraft Evren, iki dil, açıklamalı düğmeler (2026-11-05)

- Fork adı **PhotoCraft Evren**, depo `evrenot3d/photocraft` (`evren::FORK_REPO`). Güncelleme denetimi
  (`lib.rs` `UPDATE_APP`: repo, name, developer) ve Yardım bağlantıları (`links.rs`: website, GitHub, issues) forka
  bakar: resmi PhotoCraft güncellemesi eklentileri silmesin. Discord ve "ArtCraft Website" teşekkür olarak kalır.
- ArtCraft logoları silindi (`docs/brand/LICENSE-brand.txt` forkların kaldırmasını istiyor; lisans metni kaldı).
  Orijinal README `docs/PHOTOCRAFT_README.md`. NOTICE'a fork / değişiklik notu, ATTRIBUTION satırı güncellendi.
  Güncelleme alırken README.md / NOTICE çakışırsa bizimki kalır (`git checkout --ours README.md NOTICE`).
- **İki dil**: `evren/dil.rs` — `t!("English", "Türkçe")`; PhotoCraft arayüz dili Türkçeyse Türkçe, değilse
  İngilizce. `evren_ai\ayarlar.json` → `"eklenti_dili": "tr" | "en"` yalnız eklentiler için zorlar (kullanıcının
  kendi bilgisayarında "tr": arayüzü İngilizce ama İngilizce bilmiyor). Belge komutları (hata, geçmiş adı, katman
  adı) `evren_ai_cmds::t / turkce / dil_ayarla` ile (arayüz her karede ayarlar); menü etiketleri İngilizce. Python
  (`sunucu.py`, `motor.py`): her istekte `"dil"` gelir, `_t("English", "Türkçe")`. DERS: yardımcının adı `_` idi,
  `for ..., _ in` döngüleriyle çakıştı (UnboundLocalError) → `_t`. Thread içindeki metinlerde dil önceden alınır
  (i18n dili thread_local).
- **Açıklamalı düğmeler**: `arac_cubugu.rs` `DUGMELER` (id + İngilizce / Türkçe başlık ve açıklama), `on_hover_ui`.
- **Başkaları için yapay zekâ kurulumu**: `evren_ai\install.ps1` (Python 3.11 venv, torch 2.11 cu128, ComfyUI
  6e04cb2, ComfyUI-SUPIR 99d49e9, requirements.txt, `indir.py` modeller + OIDN, varsa Rust ile evren_oidn),
  `MODELS.md` (kaynaklar, boyutlar, lisanslar; bazıları ticari değil).
- README.md (İngilizce, ayrıntılı, 360 rehberi) + README.tr.md, ekran görüntüleri `docs/evren/` (İngilizce arayüz;
  telifsiz: Poly Haven CC0 panorama + gökyüzü, bina görselleri Juggernaut ile üretildi; kullanıcının işleri yok).
- Bilerek bozulan upstream testleri: `links.rs` içindeki URL testi, `wheel_nav` tekerlek testi (bkz. 7).

## Sıradaki özellikler

Kullanıcı söyledikçe eklenir; her özellik yukarıya numaralı bölüm olarak yazılır.
