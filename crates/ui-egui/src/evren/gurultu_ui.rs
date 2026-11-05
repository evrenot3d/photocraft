//! EVREN eklentisi: Gürültü temizleme (Corona Renderer'daki denoiser gibi), ekran kartında.
//!
//! Temizleyici: Intel Open Image Denoise 2.5 (açık kaynak, Apache 2.0; Corona, V-Ray ve Blender'ın da kullandığı
//! yapay zekâ gürültü temizleyicisi). NVIDIA ekran kartında CUDA ile, yoksa işlemcide çalışır. OIDN bir C kütüphanesi
//! ve PhotoCraft'ta `unsafe` yasak olduğundan iş `evren_ai\oidn\bin\evren_oidn.exe` yardımcısında yapılır
//! (kaynağı `evren_ai\oidn_yardimci`). Yardımcı yalnız bu pencere açıkken çalışır.
//!
//! Yeni katman AÇILMAZ: pencere açılınca seçili katman hemen temizlenir ve sonuç o katmanın kendi piksellerine
//! yazılır (tuvalde canlı). Miktar (Corona'daki "Denoise amount") ve Keskinlik sürüklerken de canlı güncellenir;
//! Kalite / Cihaz değişince yeniden temizlenir. Pencere boyunca yapılanlar tek geri alma adımıdır; İptal o adımı
//! geri alır, katman eski hâline döner.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use photocraft_doc::{DocId, LayerId};
use photocraft_engine::evren_ai_cmds as cmds;
use photocraft_geom::Rect;
use rayon::prelude::*;
use serde_json::{Value, json};

use super::ai_ui;
use crate::PhotocraftApp;
use crate::t;

/// Sürüklerken katman en fazla bu aralıkla yeniden yazılır (büyük resimlerde arayüz takılmasın).
const SURUKLEME_ARALIGI: Duration = Duration::from_millis(150);

// ------------------------------------------------------------------ ayarlar (evren_ai\ayarlar.json, "gurultu")

#[derive(Clone, Copy, PartialEq)]
struct Ayar {
    /// 0 otomatik, 3 ekran kartı (CUDA), 1 işlemci.
    cihaz: i32,
    /// 4 hızlı, 5 dengeli, 6 yüksek.
    kalite: i32,
    /// 0–100: temiz sonucun ne kadarı (100 = tamamen temiz).
    miktar: f32,
    /// 0–100.
    keskinlik: f32,
}

impl Default for Ayar {
    fn default() -> Self {
        Self { cihaz: 0, kalite: 6, miktar: 100.0, keskinlik: 0.0 }
    }
}

fn ayar_dosyasi() -> Option<std::path::PathBuf> {
    ai_ui::kok().map(|k| k.join("ayarlar.json"))
}

fn ayar_oku() -> Ayar {
    let d = Ayar::default();
    let Some(v) = ayar_dosyasi().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return d };
    let g = &v["gurultu"];
    let sayi = |k: &str, varsayilan: f32| g[k].as_f64().map_or(varsayilan, |x| x as f32);
    Ayar {
        cihaz: g["cihaz"].as_i64().map(|x| x as i32).filter(|x| matches!(x, 0 | 1 | 3)).unwrap_or(d.cihaz),
        kalite: g["kalite"].as_i64().map(|x| x as i32).filter(|x| (4..=6).contains(x)).unwrap_or(d.kalite),
        miktar: sayi("miktar", d.miktar).clamp(0.0, 100.0),
        keskinlik: sayi("keskinlik", d.keskinlik).clamp(0.0, 100.0),
    }
}

fn ayar_kaydet(a: &Ayar) {
    let Some(p) = ayar_dosyasi() else { return };
    let mut j = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    j["gurultu"] = json!({"cihaz": a.cihaz, "kalite": a.kalite, "miktar": a.miktar, "keskinlik": a.keskinlik});
    let _ = std::fs::write(&p, serde_json::to_vec_pretty(&j).unwrap_or_default());
}

// ------------------------------------------------------------------ yardımcı süreç (evren_oidn.exe)

struct Is {
    id: u64,
    w: u32,
    h: u32,
    rgb: Arc<Vec<u8>>,
    kalite: i32,
    cihaz: i32,
}

struct Sonuc {
    id: u64,
    /// (RGB8, cihaz adı, süre ms)
    r: Result<(Vec<u8>, String, u32), String>,
}

struct Yardimci {
    tx: Option<Sender<Is>>,
    rx: Receiver<Sonuc>,
    cocuk: Child,
}

impl Yardimci {
    fn baslat(ctx: &egui::Context) -> Result<Yardimci, String> {
        let k = ai_ui::kok().ok_or(t!("evren_ai folder not found (it must be in the PhotoCraft folder)", "evren_ai klasörü bulunamadı (PhotoCraft klasöründe olmalı)"))?;
        let exe = k.join("oidn").join("bin").join("evren_oidn.exe");
        if !exe.is_file() {
            return Err(format!("{}: {}", t!("Denoiser not found", "Gürültü temizleyici bulunamadı"), exe.display()));
        }
        let log = std::fs::File::create(k.join("oidn.log")).map_err(|e| e.to_string())?;
        let mut c = Command::new(&exe);
        if let Some(d) = exe.parent() {
            c.current_dir(d);
        }
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::from(log));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut cocuk = c.spawn().map_err(|e| format!("{}: {e}", t!("could not start the denoiser", "gürültü temizleyici açılamadı")))?;
        let mut girdi = cocuk.stdin.take().ok_or("denoiser stdin missing")?;
        let mut cikti = BufReader::with_capacity(1 << 20, cocuk.stdout.take().ok_or("denoiser stdout missing")?);
        let (tx, isler) = channel::<Is>();
        let (sonuc_tx, rx) = channel::<Sonuc>();
        let ctx = ctx.clone();
        let tr = super::dil::tr();
        std::thread::spawn(move || {
            // Kanal kapanınca (pencere kapandı) döngü biter, girdi kapanır, yardımcı çıkar.
            for is in isler {
                let r = (|| -> Result<(Vec<u8>, String, u32), String> {
                    let yaz = |e: std::io::Error| if tr { format!("gürültü temizleyiciye yazılamadı: {e}") } else { format!("could not write to the denoiser: {e}") };
                    writeln!(girdi, "TEMIZLE {} {} {} {}", is.w, is.h, is.kalite, is.cihaz).map_err(yaz)?;
                    girdi.write_all(&is.rgb).and_then(|_| girdi.flush()).map_err(yaz)?;
                    let mut satir = String::new();
                    if cikti.read_line(&mut satir).map_err(|e| e.to_string())? == 0 {
                        return Err(if tr { "gürültü temizleyici kapandı (evren_ai\\oidn.log dosyasına bakın)" } else { "the denoiser closed (see evren_ai\\oidn.log)" }.into());
                    }
                    let satir = satir.trim_end();
                    if let Some(h) = satir.strip_prefix("HATA ") {
                        return Err(h.to_string());
                    }
                    let geri = satir.strip_prefix("OK ").ok_or_else(|| format!("unexpected reply: {satir}"))?;
                    let (ms, ad) = geri.split_once(' ').unwrap_or((geri, ""));
                    let mut px = vec![0u8; is.rgb.len()];
                    cikti.read_exact(&mut px).map_err(|e| if tr { format!("sonuç okunamadı: {e}") } else { format!("could not read the result: {e}") })?;
                    Ok((px, ad.to_string(), ms.parse().unwrap_or(0)))
                })();
                let _ = sonuc_tx.send(Sonuc { id: is.id, r });
                ctx.request_repaint();
            }
        });
        Ok(Yardimci { tx: Some(tx), rx, cocuk })
    }

    fn gonder(&self, is: Is) -> Result<(), String> {
        self.tx.as_ref().ok_or(t!("the denoiser closed", "gürültü temizleyici kapandı"))?.send(is).map_err(|_| t!("the denoiser closed", "gürültü temizleyici kapandı").to_string())
    }

    /// Girdi kapanınca yardımcı kendiliğinden çıkar; 10 sn içinde çıkmazsa (uzun bir iş sürüyorsa) kapatılır.
    fn kapat(mut self) {
        drop(self.tx.take());
        std::thread::spawn(move || {
            for _ in 0..40 {
                if matches!(self.cocuk.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            let _ = self.cocuk.kill();
            let _ = self.cocuk.wait();
        });
    }
}

// ------------------------------------------------------------------ durum

struct Gurultu {
    yardimci: Yardimci,
    doc: DocId,
    /// Pencere boyunca yapılanları tek geri alma adımında birleştiren anahtar.
    anahtar: String,
    /// Temizlenen katman (pencere açıldığında seçili olan).
    katman: u64,
    katman_adi: String,
    /// Katmanın dolu bölgesi ve kanal sayısı (RGB + varsa saydamlık).
    bolge: Rect,
    kanal: usize,
    /// Katmanın özgün pikselleri (0–1, katmanın kanal düzeninde).
    orijinal: Arc<Vec<f32>>,
    /// Temizleyiciye giden RGB8.
    rgb: Arc<Vec<u8>>,
    /// Temizlenmiş RGB8.
    temiz: Option<Arc<Vec<u8>>>,
    sayac: u64,
    bekleyen: Option<u64>,
    hesap_kirli: bool,
    katman_kirli: bool,
    surukleniyor: bool,
    son_yazim: Option<Instant>,
    /// Katmana en az bir kez yazıldı (İptal'de geri alınacak).
    yazildi: bool,
    onizleme: bool,
    cihaz_adi: String,
    ms: u32,
    hata: Option<String>,
}

thread_local! {
    static DURUM: RefCell<Option<Box<Gurultu>>> = const { RefCell::new(None) };
    static AYAR: RefCell<Option<Ayar>> = const { RefCell::new(None) };
}

fn ayar() -> Ayar {
    AYAR.with(|a| *a.borrow_mut().get_or_insert_with(ayar_oku))
}

fn ayar_yaz(yeni: Ayar) {
    AYAR.with(|a| *a.borrow_mut() = Some(yeni));
}

/// Pencere açık mı (sol alttaki düğme seçili görünür).
pub fn acik() -> bool {
    DURUM.with(|d| d.borrow().is_some())
}

/// Sol alttaki düğme: seçili katman için pencereyi açar; açıksa uygulayıp kapatır.
pub fn ac(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if let Some(g) = DURUM.with(|d| d.borrow_mut().take()) {
        bitir(app, g, true);
        return;
    }
    let Some(st) = app.session.active() else {
        ai_ui::durum_yaz(app, t!("Open an image first", "Önce bir resim açın"), true);
        return;
    };
    let doc = st.doc.id;
    let Some(katman) = st.active_layer.map(|l| l.0) else {
        ai_ui::durum_yaz(app, t!("Select the layer to denoise first", "Önce temizlenecek katmanı seçin"), true);
        return;
    };
    let (katman_adi, bolge, kanal, orijinal) = match cmds::katman_kaynak(&app.session, katman) {
        Ok(k) => k,
        Err(e) => {
            ai_ui::durum_yaz(app, format!("{}: {e}", t!("Denoise", "Gürültü temizleme")), true);
            return;
        }
    };
    let yardimci = match Yardimci::baslat(ctx) {
        Ok(y) => y,
        Err(e) => {
            ai_ui::durum_yaz(app, e, true);
            return;
        }
    };
    let rgb: Vec<u8> = orijinal.par_chunks_exact(kanal).flat_map_iter(|p| [q8(p[0]), q8(p[1]), q8(p[2])]).collect();
    let g = Gurultu {
        yardimci,
        doc,
        anahtar: format!("evren-gurultu-{}", LayerId::fresh().0),
        katman,
        katman_adi,
        bolge,
        kanal,
        orijinal: Arc::new(orijinal),
        rgb: Arc::new(rgb),
        temiz: None,
        sayac: 0,
        bekleyen: None,
        hesap_kirli: true,
        katman_kirli: false,
        surukleniyor: false,
        son_yazim: None,
        yazildi: false,
        onizleme: true,
        cihaz_adi: String::new(),
        ms: 0,
        hata: None,
    };
    DURUM.with(|d| *d.borrow_mut() = Some(Box::new(g)));
}

fn q8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Kapatır. `uygula`: katman temizlenmiş kalır; değilse pencerenin geri alma adımı geri alınır.
fn bitir(app: &mut PhotocraftApp, g: Box<Gurultu>, uygula: bool) {
    let Gurultu { yardimci, doc, anahtar, katman, katman_adi, bolge, orijinal, yazildi, .. } = *g;
    yardimci.kapat();
    ayar_kaydet(&ayar());
    if uygula {
        if yazildi {
            ai_ui::durum_yaz(app, format!("{}: \"{katman_adi}\"", t!("Denoised", "Gürültü temizlendi")), false);
        }
        return;
    }
    if yazildi && ai_ui::etkinlestir(app, doc) {
        let bizim = app.session.active().is_some_and(|st| st.coalesce.as_deref() == Some(anahtar.as_str()));
        let r = if bizim {
            app.run("edit.undo", json!({}))
        } else {
            // Arada başka bir adım var: katmanın özgün pikselleri geri yazılır.
            cmds::piksel_birak(bolge, (*orijinal).clone());
            app.run(cmds::DENOISE_APPLY, json!({"coalesce": anahtar, "katman": katman}))
        };
        if let Err(e) = r {
            ai_ui::durum_yaz(app, e, true);
            return;
        }
    }
    ai_ui::durum_yaz(app, t!("Denoise cancelled", "Gürültü temizleme iptal edildi"), false);
}

// ------------------------------------------------------------------ her kare

/// Pencere ve yardımcı cevapları (her karede, evren::pencereler).
pub fn pencere(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let Some(mut g) = DURUM.with(|d| d.borrow_mut().take()) else { return };
    if !app.session.documents().iter().any(|st| st.doc.id == g.doc) {
        // Belge kapatıldı.
        g.yardimci.kapat();
        return;
    }
    while let Ok(s) = g.yardimci.rx.try_recv() {
        if g.bekleyen != Some(s.id) {
            continue;
        }
        g.bekleyen = None;
        match s.r {
            Ok((px, ad, ms)) => {
                g.temiz = Some(Arc::new(px));
                g.cihaz_adi = ad;
                g.ms = ms;
                g.hata = None;
                g.katman_kirli = true;
            }
            Err(e) => g.hata = Some(e),
        }
    }

    let mut a = ayar();
    let once = a;
    let mut acik = true;
    let mut uygula = false;
    let mut iptal = false;
    ai_ui::pencere_ac(t!("Denoise", "Gürültü temizleme"), "evren-gurultu").open(&mut acik).default_width(340.0).show(ctx, |ui| {
        ui.label(egui::RichText::new(t!("Intel Open Image Denoise · the AI denoiser renderers like Corona use", "Intel Open Image Denoise · Corona'daki gibi yapay zekâ gürültü temizleyici")).small().weak());
        ui.add_space(4.0);
        let mut surukleniyor = false;
        egui::Grid::new("evren-gurultu-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            ui.label(t!("Layer", "Katman"));
            ui.label(egui::RichText::new(&g.katman_adi).strong());
            ui.end_row();

            ui.label(t!("Device", "Cihaz"));
            let ad = |c: i32| match c {
                3 => t!("Graphics card (NVIDIA)", "Ekran kartı (NVIDIA)"),
                1 => t!("Processor (CPU)", "İşlemci"),
                _ => t!("Automatic", "Otomatik"),
            };
            egui::ComboBox::from_id_salt("evren-gurultu-cihaz").selected_text(ad(a.cihaz)).show_ui(ui, |ui| {
                for c in [0, 3, 1] {
                    if ui.selectable_label(a.cihaz == c, ad(c)).clicked() && a.cihaz != c {
                        a.cihaz = c;
                        g.hesap_kirli = true;
                    }
                }
            });
            ui.end_row();

            ui.label(t!("Quality", "Kalite"));
            ui.horizontal(|ui| {
                for (k, t) in [(4, t!("Fast", "Hızlı")), (5, t!("Balanced", "Dengeli")), (6, t!("High", "Yüksek"))] {
                    if ui.selectable_label(a.kalite == k, t).clicked() && a.kalite != k {
                        a.kalite = k;
                        g.hesap_kirli = true;
                    }
                }
            });
            ui.end_row();

            ui.label(t!("Amount", "Miktar"));
            let r = ui.add(egui::Slider::new(&mut a.miktar, 0.0..=100.0).max_decimals(0).suffix(" %"));
            g.katman_kirli |= r.changed() || r.drag_stopped();
            surukleniyor |= r.dragged();
            ui.end_row();

            ui.label(t!("Sharpness", "Keskinlik"));
            let r = ui.add(egui::Slider::new(&mut a.keskinlik, 0.0..=100.0).max_decimals(0));
            g.katman_kirli |= r.changed() || r.drag_stopped();
            surukleniyor |= r.dragged();
            ui.end_row();
        });
        g.surukleniyor = surukleniyor;
        if ui.checkbox(&mut g.onizleme, t!("Preview (off shows the original)", "Önizleme (kapatınca öncesi görünür)")).changed() {
            g.katman_kirli = true;
        }
        ui.add_space(2.0);
        if let Some(h) = &g.hata {
            ui.colored_label(ui.visuals().error_fg_color, h);
        } else if g.bekleyen.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(if g.cihaz_adi.is_empty() { t!("Denoising (the first run prepares the graphics card)…", "Temizleniyor (ilk açılışta ekran kartı hazırlanıyor)…").to_string() } else { format!("{} · {}", t!("Denoising", "Temizleniyor"), cihaz_adi(&g.cihaz_adi)) });
            });
        } else if g.temiz.is_some() {
            ui.label(egui::RichText::new(t!(format!("Ready · {:.2} s · {}", g.ms as f32 / 1000.0, cihaz_adi(&g.cihaz_adi)), format!("Hazır · {:.2} sn · {}", g.ms as f32 / 1000.0, cihaz_adi(&g.cihaz_adi)).replace('.', ","))).weak());
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(g.yazildi, egui::Button::new(t!("Apply", "Uygula"))).clicked() {
                uygula = true;
            }
            if ui.button(t!("Cancel", "İptal")).clicked() {
                iptal = true;
            }
        });
    });
    if a != once {
        ayar_yaz(a);
    }
    if iptal {
        bitir(app, g, false);
        return;
    }
    if uygula || !acik {
        bitir(app, g, true);
        return;
    }
    if let Err(e) = isle(app, ctx, &mut g, &a) {
        g.hata = Some(e);
    }
    DURUM.with(|d| *d.borrow_mut() = Some(g));
}

/// Bekleyen işler: temizlemeye gönder, sonucu katmana yaz.
fn isle(app: &mut PhotocraftApp, ctx: &egui::Context, g: &mut Gurultu, a: &Ayar) -> Result<(), String> {
    if g.hesap_kirli && g.bekleyen.is_none() {
        g.hesap_kirli = false;
        g.sayac += 1;
        g.yardimci.gonder(Is { id: g.sayac, w: g.bolge.width(), h: g.bolge.height(), rgb: g.rgb.clone(), kalite: a.kalite, cihaz: a.cihaz })?;
        g.bekleyen = Some(g.sayac);
        g.hata = None;
    }
    let Some(temiz) = g.temiz.clone() else { return Ok(()) };
    if !g.katman_kirli {
        return Ok(());
    }
    // Sürüklerken büyük resimde arayüz takılmasın: en fazla SURUKLEME_ARALIGI'nda bir yazılır.
    if g.surukleniyor
        && let Some(t) = g.son_yazim
        && t.elapsed() < SURUKLEME_ARALIGI
    {
        ctx.request_repaint_after(SURUKLEME_ARALIGI - t.elapsed());
        return Ok(());
    }
    if !ai_ui::etkinlestir(app, g.doc) {
        return Ok(());
    }
    g.katman_kirli = false;
    g.son_yazim = Some(Instant::now());
    let veri = if g.onizleme { karistir(g, &temiz, a) } else { (*g.orijinal).clone() };
    cmds::piksel_birak(g.bolge, veri);
    app.run(cmds::DENOISE_APPLY, json!({"coalesce": g.anahtar, "katman": g.katman}))?;
    g.yazildi = true;
    Ok(())
}

/// Katmana yazılacak pikseller: özgün ile (keskinleştirilmiş) temiz sonucun Miktar kadar karışımı. Saydamlık
/// kanalı özgün kalır.
fn karistir(g: &Gurultu, temiz: &[u8], a: &Ayar) -> Vec<f32> {
    let (w, h) = (g.bolge.width() as usize, g.bolge.height() as usize);
    let keskin = keskinlestir(temiz, w, h, a.keskinlik);
    let m = a.miktar / 100.0;
    let mut out = (*g.orijinal).clone();
    out.par_chunks_exact_mut(g.kanal).zip(keskin.par_chunks_exact(3)).for_each(|(o, d)| {
        for c in 0..3 {
            o[c] += m * (f32::from(d[c]) / 255.0 - o[c]);
        }
    });
    out
}

/// Yardımcının bildirdiği cihaz adı (İngilizce) Türkçe arayüzde Türkçe.
fn cihaz_adi(ad: &str) -> String {
    if super::dil::tr() { ad.replace("Graphics card", "Ekran kartı").replace("Processor", "İşlemci") } else { ad.to_string() }
}

/// Hafif keskinleştirme (3×3 bulanıklığa göre unsharp mask). `miktar` 0–100.
fn keskinlestir(rgb: &[u8], w: usize, h: usize, miktar: f32) -> std::borrow::Cow<'_, [u8]> {
    if miktar <= 0.0 || w == 0 || h == 0 {
        return std::borrow::Cow::Borrowed(rgb);
    }
    let k = miktar / 100.0 * 1.5;
    let mut out = vec![0u8; rgb.len()];
    out.par_chunks_mut(w * 3).enumerate().for_each(|(y, satir)| {
        let ys = [y.saturating_sub(1), y, (y + 1).min(h - 1)];
        for x in 0..w {
            let xs = [x.saturating_sub(1), x, (x + 1).min(w - 1)];
            for c in 0..3 {
                let mut toplam = 0u32;
                for (j, yy) in ys.iter().enumerate() {
                    for (i, xx) in xs.iter().enumerate() {
                        toplam += [1u32, 2, 1][i] * [1u32, 2, 1][j] * u32::from(rgb[(yy * w + xx) * 3 + c]);
                    }
                }
                let v = f32::from(rgb[(y * w + x) * 3 + c]);
                let bulanik = toplam as f32 / 16.0;
                satir[x * 3 + c] = (v + k * (v - bulanik)).round().clamp(0.0, 255.0) as u8;
            }
        }
    });
    std::borrow::Cow::Owned(out)
}
