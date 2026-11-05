//! EVREN eklentisi: yapay zekâ araçları: Upscale (SUPIR / UltraSharp), Alan derinliği, Bitkileri ayır,
//! Camları ayır, Gökyüzü değiştir.
//!
//! İşi `evren_ai\sunucu.py` (Python, ComfyUI) yapar. Süreç YALNIZ araç çalışırken açıktır: Upscale / Bitki / Cam
//! işi bitince, Alan derinliği ve Gökyüzü penceresi kapanınca kapatılır (ComfyUI da onunla kapanır).
//! Konuşma: süreçle satır satır JSON (sunucu.py'nin başındaki açıklama).

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use photocraft_doc::DocId;
use photocraft_engine::evren_ai_cmds as cmds;
use serde_json::{Value, json};

use crate::{PhotocraftApp, t};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arac {
    Upscale,
    Dof,
    Bitki,
    Cam,
    Gok,
    /// İnsanları düzelt (Detailer): seçim varsa yalnız seçili alan.
    Insan,
}

// ------------------------------------------------------------------ evren_ai klasörü ve süreç

/// `evren_ai` klasörü: EVREN_AI ortam değişkeni ya da programın üst klasörlerinden biri (target\release'ten yukarı).
pub fn kok() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("EVREN_AI").map(PathBuf::from).filter(|d| d.join("sunucu.py").is_file()) {
        return Some(d);
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().map(|a| a.join("evren_ai")).find(|d| d.join("sunucu.py").is_file())
}

fn calisma() -> Option<PathBuf> {
    let d = kok()?.join("calisma").join("pc");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

struct Motor {
    cocuk: Child,
    girdi: Option<ChildStdin>,
    rx: Receiver<Value>,
    sayac: u64,
}

impl Motor {
    fn baslat(ctx: &egui::Context) -> Result<Motor, String> {
        let k = kok().ok_or(t!("evren_ai folder not found (it must be in the PhotoCraft folder; see INSTALL_AI.md)", "evren_ai klasörü bulunamadı (PhotoCraft klasöründe olmalı; KURULUM_AI.md'ye bakın)"))?;
        let py = k.join(".venv").join("Scripts").join("python.exe");
        if !py.is_file() {
            return Err(format!("{}: {}", t!("Python not found; run evren_ai\\install.ps1", "Python bulunamadı; evren_ai\\install.ps1 betiğini çalıştırın"), py.display()));
        }
        let log = std::fs::File::create(k.join("sunucu.log")).map_err(|e| e.to_string())?;
        let mut c = Command::new(py);
        c.arg("-u").arg(k.join("sunucu.py")).current_dir(&k);
        c.env("PYTHONUTF8", "1").env("PYTHONIOENCODING", "utf-8");
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::from(log));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut cocuk = c.spawn().map_err(|e| format!("{}: {e}", t!("could not start the AI engine", "yapay zekâ motoru açılamadı")))?;
        let cikti = cocuk.stdout.take().ok_or("engine stdout missing")?;
        let girdi = cocuk.stdin.take();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for satir in BufReader::new(cikti).lines() {
                let Ok(satir) = satir else { break };
                if let Ok(v) = serde_json::from_str::<Value>(&satir) {
                    let _ = tx.send(v);
                    ctx.request_repaint();
                }
            }
            let _ = tx.send(json!({"durum": "bitti"}));
            ctx.request_repaint();
        });
        Ok(Motor { cocuk, girdi, rx, sayac: 0 })
    }

    fn gonder(&mut self, is: &str, mut v: Value) -> Result<u64, String> {
        self.sayac += 1;
        if let Some(o) = v.as_object_mut() {
            o.insert("id".into(), json!(self.sayac));
            o.insert("is".into(), json!(is));
            o.insert("dil".into(), json!(super::dil::kod()));
        }
        let g = self.girdi.as_mut().ok_or(t!("the AI engine closed", "motor kapandı"))?;
        writeln!(g, "{v}").and_then(|_| g.flush()).map_err(|e| format!("{}: {e}", t!("could not write to the AI engine", "motora yazılamadı")))?;
        Ok(self.sayac)
    }

    fn mesajlar(&self) -> Vec<Value> {
        self.rx.try_iter().collect()
    }

    /// Nazikçe kapatır: girdi kapanınca sunucu ComfyUI'yi kapatıp çıkar; 20 sn'de çıkmazsa öldürülür.
    fn kapat(mut self) {
        drop(self.girdi.take());
        std::thread::spawn(move || {
            for _ in 0..80 {
                if matches!(self.cocuk.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            agaci_oldur(&mut self.cocuk);
        });
    }

    /// İptal: süreç ağacı (ComfyUI dahil) hemen kapatılır.
    fn oldur(mut self) {
        agaci_oldur(&mut self.cocuk);
    }
}

fn agaci_oldur(c: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &c.id().to_string()]).creation_flags(0x0800_0000).status();
    }
    let _ = c.kill();
    let _ = c.wait();
}

// ------------------------------------------------------------------ durum

#[derive(Clone, Copy, PartialEq)]
enum UpModel {
    Supir,
    Hizli,
}

struct UpAyar {
    model: UpModel,
    olcek: f32,
    adim: u32,
    tumu: bool,
    secili: Vec<DocId>,
}

impl Default for UpAyar {
    fn default() -> Self {
        Self { model: UpModel::Supir, olcek: 2.0, adim: 30, tumu: false, secili: Vec::new() }
    }
}

struct UpIs {
    doc: DocId,
    ad: String,
    girdi: PathBuf,
    w: u32,
    h: u32,
}

struct UpCalisma {
    motor: Motor,
    kuyruk: Vec<UpIs>,
    sira: usize,
    bekleyen: u64,
    asama: String,
    yuzde: Option<f32>,
    tamam: usize,
    model_ad: String,
}

struct Onizleme {
    motor: Motor,
    doc: DocId,
    girdi: PathBuf,
    hazir: bool,
    kirli: bool,
    bekleyen: Option<(u64, bool)>, // (id, tam çözünürlük mü)
    doku: Option<egui::TextureHandle>,
    asama: String,
    hata: Option<String>,
}

struct DofAyar {
    fx: f32,
    fy: f32,
    miktar: f32,
    aralik: f32,
    yon: &'static str,
}

impl Default for DofAyar {
    fn default() -> Self {
        Self { fx: 0.5, fy: 0.5, miktar: 40.0, aralik: 15.0, yon: "iki" }
    }
}

const GOK_AYARLARI: &[(&str, &str, f32, f32, f32)] = &[
    ("Kenarı kaydır", "kenar", -30.0, 30.0, 0.0),
    ("Yumuşaklık", "yumusak", 0.0, 30.0, 3.0),
    ("Ölçek", "olcek", 50.0, 300.0, 100.0),
    ("Yatay konum", "kx", -100.0, 100.0, 0.0),
    ("Dikey konum", "ky", -100.0, 100.0, 0.0),
    ("Parlaklık", "parlak", -100.0, 100.0, 0.0),
    ("Sıcaklık", "sicaklik", -100.0, 100.0, 0.0),
    ("Ön plan uyumu", "uyum", 0.0, 100.0, 50.0),
    ("Kenar ışığı", "isik", 0.0, 100.0, 20.0),
];

struct GokResim {
    ad: String,
    yol: String,
    _kucuk: String,
    doku: Option<egui::TextureHandle>,
}

struct GokDurum {
    on: Onizleme,
    liste: Vec<GokResim>,
    liste_istek: Option<u64>,
    secili: usize,
    ayar: Vec<f32>,
    eklenecek: Option<String>,
}

struct AyirDurum {
    motor: Motor,
    doc: DocId,
    tur: Arac,
    bekleyen: u64,
    asama: String,
    yuzde: Option<f32>,
    /// Pencerede gösterilen ek bilgi (İnsanları düzelt: hangi alan).
    not: String,
}

/// Bitki / Cam / İnsan işlerinin (katman adı, pencere başlığı).
fn ayir_adlari(tur: Arac) -> (&'static str, &'static str) {
    match tur {
        Arac::Bitki => (t!("Plants (separated)", "Bitkiler (ayrıldı)"), t!("Separate Plants", "Bitkileri ayır")),
        Arac::Insan => (t!("People (fixed)", "İnsanlar (düzeltildi)"), t!("Fix People", "İnsanları düzelt")),
        _ => (t!("Glass (separated)", "Camlar (ayrıldı)"), t!("Separate Glass", "Camları ayır")),
    }
}

/// Etkin belgedeki seçimin sınır kutusu (belgenin içinde kalan kısmı); seçim yoksa None.
fn secim_kutusu(app: &PhotocraftApp) -> Option<photocraft_geom::Rect> {
    let st = app.session.active()?;
    let r = st.doc.selection.as_ref()?.content_bounds().intersect(&st.doc.bounds());
    (!r.is_empty()).then_some(r)
}

#[derive(Default)]
enum Durum {
    #[default]
    Yok,
    UpPencere,
    UpCalisiyor(Box<UpCalisma>),
    Dof(Box<Onizleme>),
    Gok(Box<GokDurum>),
    Ayir(Box<AyirDurum>),
}

#[derive(Default)]
struct Hepsi {
    durum: Durum,
    up: UpAyar,
    dof: DofAyar,
    gok_secili_ad: Option<String>,
    gok_ayar: Option<Vec<f32>>,
    gok_eklenecek: Option<String>,
}

thread_local! {
    static HEPSI: RefCell<Hepsi> = RefCell::new(Hepsi::default());
}

/// Araç açık / çalışıyor mu (düğme seçili görünür).
pub fn acik(arac: Arac) -> bool {
    HEPSI.with(|h| {
        matches!(
            (&h.borrow().durum, arac),
            (Durum::UpPencere | Durum::UpCalisiyor(_), Arac::Upscale) | (Durum::Dof(_), Arac::Dof) | (Durum::Gok(_), Arac::Gok)
        ) || matches!(&h.borrow().durum, Durum::Ayir(a) if a.tur == arac)
    })
}

pub(super) fn durum_yaz(app: &mut PhotocraftApp, mesaj: impl Into<String>, hata: bool) {
    app.ui.status = mesaj.into();
    app.ui.status_error = hata;
}

fn etkin_doc(app: &PhotocraftApp) -> Option<(DocId, String, u32, u32)> {
    let st = app.session.active()?;
    Some((st.doc.id, st.doc.name.clone(), st.doc.size.width, st.doc.size.height))
}

/// Belgeyi etkin sekme yapar (açık değilse false).
pub(super) fn etkinlestir(app: &mut PhotocraftApp, doc: DocId) -> bool {
    if app.session.active().is_some_and(|st| st.doc.id == doc) {
        return true;
    }
    match app.session.documents().iter().position(|st| st.doc.id == doc) {
        Some(i) => app.session.set_active(i),
        None => false,
    }
}

/// Belgenin görünen hâli PNG'ye.
fn goruntu_yaz(app: &mut PhotocraftApp, doc: DocId, beyaz: bool) -> Result<PathBuf, String> {
    let yol = calisma().ok_or(t!("could not create evren_ai\\calisma", "evren_ai\\calisma klasörü açılamadı"))?.join(format!("{}_girdi.png", doc.0));
    app.run(cmds::COMPOSITE, json!({"path": yol.to_string_lossy(), "white": beyaz}))?;
    Ok(yol)
}

fn cikti_yolu(doc: DocId, ek: &str) -> String {
    calisma().map(|d| d.join(format!("{}_{ek}.png", doc.0)).to_string_lossy().into_owned()).unwrap_or_default()
}

pub(super) fn doku_yukle(ctx: &egui::Context, yol: &str, ad: &str) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(yol).ok()?;
    let img = photocraft_codecs::decode(&bytes).ok()?;
    let (w, h) = img.dimensions();
    let rgba = img.to_rgba8();
    let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
    Some(ctx.load_texture(ad, ci, egui::TextureOptions::LINEAR))
}

/// Hedef boyut (8'in katı).
fn hedef_boyut(w: u32, h: u32, k: f32) -> (u32, u32) {
    let f = |v: u32| ((((v as f32) * k / 8.0).round() as u32) * 8).max(8);
    (f(w), f(h))
}

const MAX_KENAR: u32 = 16000;
const MAX_PIKSEL: u64 = 250_000_000;

// ------------------------------------------------------------------ açma

/// Sol menüden bir araç (açıksa kapatır).
pub fn ac(app: &mut PhotocraftApp, ctx: &egui::Context, arac: Arac) {
    if acik(arac) {
        kapat(app, false);
        return;
    }
    let mesgul = HEPSI.with(|h| !matches!(h.borrow().durum, Durum::Yok | Durum::UpPencere));
    if mesgul {
        durum_yaz(app, t!("An AI job is running; finish or close it first", "Bir yapay zekâ işi sürüyor; önce onu bitirin ya da kapatın"), true);
        return;
    }
    let Some((doc, _, _, _)) = etkin_doc(app) else {
        durum_yaz(app, t!("Open an image first", "Önce bir resim açın"), true);
        return;
    };
    let yeni = match arac {
        Arac::Upscale => Ok(Durum::UpPencere),
        Arac::Dof | Arac::Gok | Arac::Bitki | Arac::Cam | Arac::Insan => (|| -> Result<Durum, String> {
            // İnsanları düzelt: seçim varsa yalnız onun kutusu (ör. dikdörtgen seçim), yoksa bütün resim.
            let bolge = if arac == Arac::Insan { secim_kutusu(app) } else { None };
            let girdi = goruntu_yaz(app, doc, false)?;
            let mut motor = Motor::baslat(ctx)?;
            let gs = girdi.to_string_lossy().into_owned();
            Ok(match arac {
                Arac::Dof => {
                    let id = motor.gonder("derinlik", json!({"girdi": gs}))?;
                    Durum::Dof(Box::new(Onizleme { motor, doc, girdi, hazir: false, kirli: true, bekleyen: Some((id, false)), doku: None, asama: t!("Computing depth", "Derinlik hesaplanıyor").into(), hata: None }))
                }
                Arac::Gok => {
                    let id = motor.gonder("gok_liste", json!({}))?;
                    let (secili_ad, ayar) = HEPSI.with(|h| (h.borrow().gok_secili_ad.clone(), h.borrow().gok_ayar.clone()));
                    let _ = secili_ad;
                    let on = Onizleme { motor, doc, girdi, hazir: false, kirli: false, bekleyen: None, doku: None, asama: t!("Loading the sky library", "Gökyüzü kütüphanesi yükleniyor").into(), hata: None };
                    Durum::Gok(Box::new(GokDurum {
                        on,
                        liste: Vec::new(),
                        liste_istek: Some(id),
                        secili: 0,
                        ayar: ayar.unwrap_or_else(|| GOK_AYARLARI.iter().map(|a| a.4).collect()),
                        eklenecek: None,
                    }))
                }
                Arac::Insan => {
                    let (b, not) = match bolge {
                        Some(r) => (json!([r.x0, r.y0, r.width(), r.height()]), t!(format!("Selected area: {} × {} pixels", r.width(), r.height()), format!("Seçili alan: {} × {} piksel", r.width(), r.height()))),
                        None => (Value::Null, t!("No selection: the whole image", "Seçim yok: bütün görsel").to_string()),
                    };
                    let id = motor.gonder("insan", json!({"girdi": gs, "cikti": cikti_yolu(doc, "insan"), "bolge": b, "guc": 0.5}))?;
                    Durum::Ayir(Box::new(AyirDurum { motor, doc, tur: arac, bekleyen: id, asama: t!("Starting", "Başlatılıyor").into(), yuzde: None, not }))
                }
                _ => {
                    let tur = if arac == Arac::Bitki { "bitki" } else { "cam" };
                    let id = motor.gonder("ayir", json!({"tur": tur, "girdi": gs, "cikti": cikti_yolu(doc, tur)}))?;
                    Durum::Ayir(Box::new(AyirDurum { motor, doc, tur: arac, bekleyen: id, asama: t!("Starting", "Başlatılıyor").into(), yuzde: None, not: String::new() }))
                }
            })
        })(),
    };
    match yeni {
        Ok(d) => HEPSI.with(|h| h.borrow_mut().durum = d),
        Err(e) => durum_yaz(app, e, true),
    }
}

/// Açık aracı kapatır (`iptal`: süreç hemen öldürülür).
fn kapat(app: &mut PhotocraftApp, iptal: bool) {
    let d = HEPSI.with(|h| std::mem::take(&mut h.borrow_mut().durum));
    let motor = match d {
        Durum::UpCalisiyor(u) => Some(u.motor),
        Durum::Dof(o) => Some(o.motor),
        Durum::Gok(g) => Some(g.on.motor),
        Durum::Ayir(a) => Some(a.motor),
        Durum::Yok | Durum::UpPencere => None,
    };
    if let Some(m) = motor {
        if iptal {
            m.oldur();
            durum_yaz(app, t!("AI job cancelled", "Yapay zekâ işi iptal edildi"), false);
        } else {
            m.kapat();
        }
    }
}

// ------------------------------------------------------------------ her kare

/// Pencereler ve süreç mesajları (her karede, app/frame.rs → evren::pencereler).
pub fn pencere(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let mut d = HEPSI.with(|h| std::mem::take(&mut h.borrow_mut().durum));
    d = match d {
        Durum::Yok => Durum::Yok,
        Durum::UpPencere => up_pencere(app, ctx),
        Durum::UpCalisiyor(u) => up_calisiyor(app, ctx, u),
        Durum::Dof(o) => dof_pencere(app, ctx, o),
        Durum::Gok(g) => gok_pencere(app, ctx, g),
        Durum::Ayir(a) => ayir_pencere(app, ctx, a),
    };
    HEPSI.with(|h| h.borrow_mut().durum = d);
}

pub(super) fn pencere_ac<'a>(baslik: &str, id: &str) -> egui::Window<'a> {
    egui::Window::new(baslik.to_string()).id(egui::Id::unique(id)).resizable(false).collapsible(false).default_pos(egui::pos2(90.0, 120.0))
}

fn ilerleme_satiri(ui: &mut egui::Ui, asama: &str, yuzde: Option<f32>) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(asama);
    });
    if let Some(y) = yuzde {
        ui.add(egui::ProgressBar::new(y.clamp(0.0, 1.0)).show_percentage());
    }
}

/// Bir motor cevabının ortak alanları: (id, durum, mesaj).
fn cevap(v: &Value) -> (Option<u64>, &str, Option<String>) {
    (v["id"].as_u64(), v["durum"].as_str().unwrap_or(""), v["mesaj"].as_str().map(str::to_string))
}

// ------------------------------------------------------------------ Upscale

fn up_pencere(app: &mut PhotocraftApp, ctx: &egui::Context) -> Durum {
    let Some((doc, ad, w, h)) = etkin_doc(app) else {
        return Durum::Yok;
    };
    let belgeler: Vec<(DocId, String, u32, u32)> = app.session.documents().iter().map(|st| (st.doc.id, st.doc.name.clone(), st.doc.size.width, st.doc.size.height)).collect();
    let mut acik = true;
    let mut basla: Option<Vec<DocId>> = None;
    HEPSI.with(|hh| {
        let mut hh = hh.borrow_mut();
        let a = &mut hh.up;
        a.secili.retain(|d| belgeler.iter().any(|b| b.0 == *d));
        pencere_ac("Upscale", "evren-up").open(&mut acik).default_width(330.0).show(ctx, |ui| {
            ui.strong("Model");
            ui.radio_value(&mut a.model, UpModel::Supir, t!("SUPIR  ·  adds detail, best quality (slow)", "SUPIR  ·  detay ekler, en kaliteli (yavaş)"));
            ui.radio_value(&mut a.model, UpModel::Hizli, t!("UltraSharp  ·  sharp enlargement only (fast)", "UltraSharp  ·  yalnız keskin büyütür (hızlı)"));
            ui.separator();
            ui.strong(t!("Scale", "Büyütme"));
            ui.horizontal(|ui| {
                for k in [1.0f32, 1.5, 2.0, 4.0, 8.0] {
                    let etiket = if k.fract() == 0.0 { format!("{k:.0}x") } else { t!("1.5x", "1,5x").to_string() };
                    if ui.selectable_label((a.olcek - k).abs() < 1e-3, etiket).clicked() {
                        a.olcek = k;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(t!("Custom", "Özel oran"));
                ui.add(egui::DragValue::new(&mut a.olcek).range(1.0..=8.0).speed(0.05).max_decimals(2).suffix("x"));
            });
            if a.model == UpModel::Supir {
                ui.separator();
                ui.strong(t!("Steps", "Adım sayısı"));
                ui.horizontal(|ui| {
                    for n in [15u32, 20, 30, 45] {
                        if ui.selectable_label(a.adim == n, n.to_string()).clicked() {
                            a.adim = n;
                        }
                    }
                });
            }
            ui.separator();
            ui.strong(t!("Result", "Sonuç"));
            let (tw, th) = hedef_boyut(w, h, a.olcek);
            ui.label(format!("{}: {ad}\n{w}×{h} → {tw}×{th}", t!("Active tab", "Etkin sekme")));
            let buyuk = |w: u32, h: u32| w.max(h) > MAX_KENAR || u64::from(w) * u64::from(h) > MAX_PIKSEL;
            if buyuk(tw, th) {
                ui.colored_label(ui.visuals().error_fg_color, t!(format!("Too large: at most {MAX_KENAR} px per side"), format!("Çok büyük: en fazla {MAX_KENAR} px kenar")));
            }
            if a.tumu {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.strong(t!("Tabs", "Sekmeler"));
                    let hepsi = a.secili.len() == belgeler.len();
                    if ui.small_button(if hepsi { t!("None", "Hiçbiri") } else { t!("All", "Hepsi") }).clicked() {
                        a.secili = if hepsi { Vec::new() } else { belgeler.iter().map(|b| b.0).collect() };
                    }
                });
                for (id, ad, bw, bh) in &belgeler {
                    let mut on = a.secili.contains(id);
                    let (tw, th) = hedef_boyut(*bw, *bh, a.olcek);
                    if ui.checkbox(&mut on, format!("{ad}   {bw}×{bh} → {tw}×{th}")).changed() {
                        if on {
                            a.secili.push(*id);
                        } else {
                            a.secili.retain(|d| d != id);
                        }
                    }
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if !a.tumu && ui.button(t!("Apply to all tabs…", "Tümüne uygula…")).clicked() {
                    a.tumu = true;
                    a.secili = belgeler.iter().map(|b| b.0).collect();
                }
                if a.tumu && ui.button(t!("This tab only", "Yalnız bu sekme")).clicked() {
                    a.tumu = false;
                }
                let etiket = if a.tumu { t!(format!("Apply to selected ({})", a.secili.len()), format!("Seçilenlere uygula ({})", a.secili.len())) } else { t!("Apply", "Uygula").to_string() };
                let olur = if a.tumu { !a.secili.is_empty() } else { !buyuk(tw, th) };
                if ui.add_enabled(olur, egui::Button::new(etiket)).clicked() {
                    basla = Some(if a.tumu { belgeler.iter().map(|b| b.0).filter(|d| a.secili.contains(d)).collect() } else { vec![doc] });
                }
            });
        });
    });
    if let Some(hedef) = basla {
        return up_baslat(app, ctx, hedef);
    }
    if acik { Durum::UpPencere } else { Durum::Yok }
}

fn up_baslat(app: &mut PhotocraftApp, ctx: &egui::Context, hedef: Vec<DocId>) -> Durum {
    let (model, olcek, adim) = HEPSI.with(|h| {
        let a = &h.borrow().up;
        (a.model, a.olcek, a.adim)
    });
    let model_ad = if model == UpModel::Supir { "SUPIR" } else { "UltraSharp" };
    let mut kuyruk = Vec::new();
    for doc in hedef {
        if !etkinlestir(app, doc) {
            continue;
        }
        let Some((_, ad, w, h)) = etkin_doc(app) else { continue };
        let (tw, th) = hedef_boyut(w, h, olcek);
        if tw.max(th) > MAX_KENAR || u64::from(tw) * u64::from(th) > MAX_PIKSEL {
            durum_yaz(app, t!(format!("{ad}: target size too large, skipped"), format!("{ad}: hedef boyut çok büyük, atlandı")), true);
            continue;
        }
        match goruntu_yaz(app, doc, true) {
            Ok(girdi) => kuyruk.push(UpIs { doc, ad, girdi, w: tw, h: th }),
            Err(e) => durum_yaz(app, e, true),
        }
    }
    if kuyruk.is_empty() {
        return Durum::UpPencere;
    }
    let mut motor = match Motor::baslat(ctx) {
        Ok(m) => m,
        Err(e) => {
            durum_yaz(app, e, true);
            return Durum::UpPencere;
        }
    };
    let ilk = &kuyruk[0];
    let istek = json!({"girdi": ilk.girdi.to_string_lossy(), "cikti": cikti_yolu(ilk.doc, "upscale"), "genislik": ilk.w, "yukseklik": ilk.h,
        "model": if model == UpModel::Supir { "supir" } else { "hizli" }, "adim": adim});
    let _ = etkinlestir(app, ilk.doc);
    match motor.gonder("upscale", istek) {
        Ok(id) => Durum::UpCalisiyor(Box::new(UpCalisma {
            motor,
            kuyruk,
            sira: 0,
            bekleyen: id,
            asama: t!("Starting the engine (ComfyUI)", "Motor (ComfyUI) açılıyor").into(),
            yuzde: None,
            tamam: 0,
            model_ad: format!("{model_ad} {}x", t!(format!("{olcek}"), format!("{olcek}").replace('.', ","))),
        })),
        Err(e) => {
            durum_yaz(app, e, true);
            motor.oldur();
            Durum::UpPencere
        }
    }
}

fn up_calisiyor(app: &mut PhotocraftApp, ctx: &egui::Context, mut u: Box<UpCalisma>) -> Durum {
    for v in u.motor.mesajlar() {
        let (id, durum, mesaj) = cevap(&v);
        if durum == "bitti" {
            durum_yaz(app, t!("The AI engine closed unexpectedly (see evren_ai\\sunucu.log)", "Yapay zekâ motoru beklenmedik şekilde kapandı (evren_ai\\sunucu.log)"), true);
            u.motor.oldur();
            return Durum::Yok;
        }
        if id != Some(u.bekleyen) {
            continue;
        }
        match durum {
            "ilerleme" => {
                if let Some(a) = v["asama"].as_str() {
                    u.asama = a.to_string();
                }
                u.yuzde = v["yuzde"].as_f64().map(|y| y as f32);
            }
            "tamam" => {
                let is = &u.kuyruk[u.sira];
                if etkinlestir(app, is.doc) {
                    let r = app.run(cmds::UPSCALE, json!({"path": v["cikti"], "name": format!("Upscale · {}", u.model_ad)}));
                    match r {
                        Ok(_) => u.tamam += 1,
                        Err(e) => durum_yaz(app, format!("{}: {e}", is.ad), true),
                    }
                }
                u.sira += 1;
                if u.sira >= u.kuyruk.len() {
                    let n = u.tamam;
                    durum_yaz(app, t!(format!("Upscale done: {n} tab(s) · {}", u.model_ad), format!("Upscale tamam: {n} sekme · {}", u.model_ad)), false);
                    u.motor.kapat();
                    return Durum::Yok;
                }
                let (model, adim) = HEPSI.with(|h| (h.borrow().up.model, h.borrow().up.adim));
                let is = &u.kuyruk[u.sira];
                let _ = etkinlestir(app, is.doc);
                let istek = json!({"girdi": is.girdi.to_string_lossy(), "cikti": cikti_yolu(is.doc, "upscale"), "genislik": is.w, "yukseklik": is.h,
                    "model": if model == UpModel::Supir { "supir" } else { "hizli" }, "adim": adim});
                match u.motor.gonder("upscale", istek) {
                    Ok(id) => {
                        u.bekleyen = id;
                        u.asama = t!("Queued", "Sırada").into();
                        u.yuzde = None;
                    }
                    Err(e) => {
                        durum_yaz(app, e, true);
                        u.motor.oldur();
                        return Durum::Yok;
                    }
                }
            }
            "hata" => {
                durum_yaz(app, format!("Upscale: {}", mesaj.unwrap_or_default()), true);
                u.motor.kapat();
                return Durum::Yok;
            }
            _ => {}
        }
    }
    let mut iptal = false;
    let is = &u.kuyruk[u.sira.min(u.kuyruk.len() - 1)];
    pencere_ac("Upscale", "evren-up-calisiyor").default_width(320.0).show(ctx, |ui| {
        let on = if u.kuyruk.len() > 1 { format!("{} {}/{} · ", t!("Tab", "Sekme"), u.sira + 1, u.kuyruk.len()) } else { String::new() };
        ui.label(format!("{on}{} · {}×{} · {}", is.ad, is.w, is.h, u.model_ad));
        ilerleme_satiri(ui, &u.asama, u.yuzde);
        if ui.button(t!("Cancel", "İptal")).clicked() {
            iptal = true;
        }
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
    if iptal {
        u.motor.oldur();
        durum_yaz(app, t!("Upscale cancelled", "Upscale iptal edildi"), false);
        return Durum::Yok;
    }
    Durum::UpCalisiyor(u)
}

// ------------------------------------------------------------------ Bitki / Cam ayır, İnsanları düzelt

fn ayir_pencere(app: &mut PhotocraftApp, ctx: &egui::Context, mut a: Box<AyirDurum>) -> Durum {
    let (ad, baslik) = ayir_adlari(a.tur);
    for v in a.motor.mesajlar() {
        let (id, durum, mesaj) = cevap(&v);
        if durum == "bitti" {
            durum_yaz(app, t!("The AI engine closed unexpectedly (see evren_ai\\sunucu.log)", "Yapay zekâ motoru beklenmedik şekilde kapandı (evren_ai\\sunucu.log)"), true);
            return Durum::Yok;
        }
        if id != Some(a.bekleyen) {
            continue;
        }
        match durum {
            "ilerleme" => {
                if let Some(s) = v["asama"].as_str() {
                    a.asama = s.to_string();
                }
                a.yuzde = v["yuzde"].as_f64().map(|y| y as f32);
            }
            "tamam" => {
                if etkinlestir(app, a.doc) {
                    let ek = match (a.tur, v["kisi"].as_u64()) {
                        (Arac::Insan, Some(n)) => {
                            let atlanan = v["atlanan"].as_u64().unwrap_or(0);
                            let yuz = v["yuz"].as_u64().unwrap_or(0);
                            let mut s = t!(format!(" ({n} people redrawn, {yuz} faces redrawn separately"), format!(" ({n} kişi baştan çizildi, {yuz} yüz ayrıca yeniden çizildi"));
                            if atlanan > 0 {
                                s += &t!(format!("; {atlanan} nearby people left untouched"), format!("; yakındaki {atlanan} kişiye dokunulmadı"));
                            }
                            if v["controlnet"].as_bool() == Some(false) {
                                s += t!("; pose model (ControlNet) not found", "; duruş modeli (ControlNet) bulunamadı");
                            }
                            s + ")"
                        }
                        _ => String::new(),
                    };
                    match app.run(cmds::ADD_LAYER, json!({"path": v["cikti"], "name": ad})) {
                        Ok(_) => durum_yaz(app, format!("{ad}: {}{ek}", t!("new layer added", "yeni katman eklendi")), false),
                        Err(e) => durum_yaz(app, e, true),
                    }
                }
                a.motor.kapat();
                return Durum::Yok;
            }
            "hata" => {
                durum_yaz(app, mesaj.unwrap_or_default(), true);
                a.motor.kapat();
                return Durum::Yok;
            }
            _ => {}
        }
    }
    let mut iptal = false;
    pencere_ac(baslik, "evren-ayir").default_width(300.0).show(ctx, |ui| {
        ilerleme_satiri(ui, &a.asama, a.yuzde);
        if !a.not.is_empty() {
            ui.label(&a.not);
        }
        ui.label(t!("The result will be added as a new layer.", "Sonuç yeni katman olarak eklenecek."));
        if ui.button(t!("Cancel", "İptal")).clicked() {
            iptal = true;
        }
    });
    if iptal {
        a.motor.oldur();
        return Durum::Yok;
    }
    Durum::Ayir(a)
}

// ------------------------------------------------------------------ önizlemeli pencereler (Alan derinliği, Gökyüzü)

/// Önizleme resmi; tıklanırsa resim içindeki konum (0–1).
fn onizleme_resmi(ui: &mut egui::Ui, doku: Option<&egui::TextureHandle>, isaret: Option<(f32, f32)>) -> Option<(f32, f32)> {
    let alan = egui::vec2(460.0, 330.0);
    let Some(t) = doku else {
        let (r, _) = ui.allocate_exact_size(alan, egui::Sense::hover());
        ui.painter().rect_filled(r, 4.0, ui.visuals().extreme_bg_color);
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, t!("Preparing preview…", "Önizleme hazırlanıyor…"), egui::FontId::proportional(13.0), ui.visuals().weak_text_color());
        return None;
    };
    let s = t.size_vec2();
    let k = (alan.x / s.x).min(alan.y / s.y);
    let boy = s * k;
    let (r, resp) = ui.allocate_exact_size(boy, egui::Sense::click());
    ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    if let Some((fx, fy)) = isaret {
        let p = egui::pos2(r.left() + fx * r.width(), r.top() + fy * r.height());
        ui.painter().circle_stroke(p, 7.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
        ui.painter().circle_stroke(p, 8.5, egui::Stroke::new(1.0, egui::Color32::BLACK));
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        return Some(((p.x - r.left()) / r.width(), (p.y - r.top()) / r.height()));
    }
    resp.on_hover_cursor(egui::CursorIcon::Crosshair);
    None
}

/// Motor cevaplarını işler. Dönüş: (önizleme yüklendi mi, tam sonuç yolu).
fn on_mesajlar(ctx: &egui::Context, o: &mut Onizleme, ad: &str) -> Result<Option<String>, String> {
    let mut tam = None;
    for v in o.motor.mesajlar() {
        let (id, durum, mesaj) = cevap(&v);
        if durum == "bitti" {
            return Err(t!("The AI engine closed unexpectedly (see evren_ai\\sunucu.log)", "Yapay zekâ motoru beklenmedik şekilde kapandı (evren_ai\\sunucu.log)").into());
        }
        let Some((bek, tam_mi)) = o.bekleyen else { continue };
        if id != Some(bek) {
            continue;
        }
        match durum {
            "ilerleme" => {
                if let Some(s) = v["asama"].as_str() {
                    o.asama = s.to_string();
                }
            }
            "tamam" => {
                o.bekleyen = None;
                o.hata = None;
                if !o.hazir {
                    o.hazir = true;
                } else if tam_mi {
                    tam = v["cikti"].as_str().map(str::to_string);
                } else if let Some(y) = v["cikti"].as_str() {
                    o.doku = doku_yukle(ctx, y, ad);
                }
            }
            "hata" => {
                o.bekleyen = None;
                o.hata = mesaj;
            }
            _ => {}
        }
    }
    Ok(tam)
}

fn dof_pencere(app: &mut PhotocraftApp, ctx: &egui::Context, mut o: Box<Onizleme>) -> Durum {
    let tam = match on_mesajlar(ctx, &mut o, "evren-dof-onizleme") {
        Ok(t) => t,
        Err(e) => {
            durum_yaz(app, e, true);
            return Durum::Yok;
        }
    };
    if let Some(yol) = tam {
        if etkinlestir(app, o.doc) {
            match app.run(cmds::ADD_LAYER, json!({"path": yol, "name": t!("Depth of Field", "Alan derinliği")})) {
                Ok(_) => durum_yaz(app, t!("Depth of Field: new layer added", "Alan derinliği: yeni katman eklendi"), false),
                Err(e) => durum_yaz(app, e, true),
            }
        }
        o.motor.kapat();
        return Durum::Yok;
    }
    let mut acik = true;
    let mut iptal_tik = false;
    let mut uygula = false;
    HEPSI.with(|hh| {
        let mut hh = hh.borrow_mut();
        let a = &mut hh.dof;
        pencere_ac(t!("Depth of Field", "Alan derinliği"), "evren-dof").open(&mut acik).default_width(480.0).show(ctx, |ui| {
            ui.label(t!("Click the spot that should stay in focus.", "Önizlemede netleşecek yere tıklayın."));
            if let Some((fx, fy)) = onizleme_resmi(ui, o.doku.as_ref(), Some((a.fx, a.fy))) {
                a.fx = fx.clamp(0.0, 1.0);
                a.fy = fy.clamp(0.0, 1.0);
                o.kirli = true;
            }
            egui::Grid::new("evren-dof-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(t!("Blur", "Bulanıklık"));
                let r = ui.add(egui::Slider::new(&mut a.miktar, 0.0..=100.0).max_decimals(0));
                o.kirli |= r.drag_stopped() || (r.changed() && !r.dragged());
                ui.end_row();
                ui.label(t!("Focus range", "Net kalan derinlik"));
                let r = ui.add(egui::Slider::new(&mut a.aralik, 0.0..=100.0).max_decimals(0));
                o.kirli |= r.drag_stopped() || (r.changed() && !r.dragged());
                ui.end_row();
                ui.label(t!("Blur where", "Bulanıklaşan yer"));
                ui.horizontal(|ui| {
                    for (k, etiket) in [("iki", t!("Front and back", "Önü ve arkası")), ("uzak", t!("Background only", "Yalnız arkası")), ("yakin", t!("Foreground only", "Yalnız önü"))] {
                        if ui.selectable_label(a.yon == k, etiket).clicked() {
                            a.yon = k;
                            o.kirli = true;
                        }
                    }
                });
                ui.end_row();
            });
            durum_satiri(ui, &o);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(o.hazir && o.bekleyen.is_none(), egui::Button::new(t!("Apply (new layer)", "Uygula (yeni katman)"))).clicked() {
                    uygula = true;
                }
                if ui.button(t!("Cancel", "İptal")).clicked() {
                    iptal_tik = true;
                }
            });
        });
    });
    if !acik || iptal_tik {
        o.motor.kapat();
        return Durum::Yok;
    }
    let a = HEPSI.with(|h| {
        let d = &h.borrow().dof;
        json!({"fx": d.fx, "fy": d.fy, "miktar": d.miktar, "aralik": d.aralik, "yon": d.yon})
    });
    let gs = o.girdi.to_string_lossy().into_owned();
    if uygula {
        o.asama = t!("Applying at full resolution", "Tam çözünürlükte uygulanıyor").into();
        match o.motor.gonder("dof", json!({"girdi": gs, "cikti": cikti_yolu(o.doc, "dof"), "ayar": a})) {
            Ok(id) => o.bekleyen = Some((id, true)),
            Err(e) => o.hata = Some(e),
        }
    } else if o.hazir && o.kirli && o.bekleyen.is_none() {
        o.kirli = false;
        o.asama = t!("Preview", "Önizleme").into();
        match o.motor.gonder("dof", json!({"girdi": gs, "cikti": cikti_yolu(o.doc, "dof_on"), "ayar": a, "onizleme": 900})) {
            Ok(id) => o.bekleyen = Some((id, false)),
            Err(e) => o.hata = Some(e),
        }
    }
    Durum::Dof(o)
}

fn durum_satiri(ui: &mut egui::Ui, o: &Onizleme) {
    if let Some(h) = &o.hata {
        ui.colored_label(ui.visuals().error_fg_color, h);
    } else if o.bekleyen.is_some() {
        ilerleme_satiri(ui, &o.asama, None);
    }
}

fn gok_pencere(app: &mut PhotocraftApp, ctx: &egui::Context, mut g: Box<GokDurum>) -> Durum {
    // kütüphane listesi cevabı ayrı ele alınır
    let mesajlar = g.on.motor.mesajlar();
    let mut kalan = Vec::new();
    for v in mesajlar {
        if v["id"].as_u64().is_some() && v["id"].as_u64() == g.liste_istek {
            match v["durum"].as_str() {
                Some("tamam") => {
                    g.liste_istek = None;
                    let eski = g.liste.get(g.secili).map(|r| r.ad.clone()).or_else(|| HEPSI.with(|h| h.borrow().gok_secili_ad.clone()));
                    g.liste = v["liste"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|r| {
                            let kucuk = r["kucuk"].as_str().unwrap_or_default().to_string();
                            GokResim {
                                ad: r["ad"].as_str().unwrap_or_default().to_string(),
                                yol: r["yol"].as_str().unwrap_or_default().to_string(),
                                doku: doku_yukle(ctx, &kucuk, &format!("evren-gok-{kucuk}")),
                                _kucuk: kucuk,
                            }
                        })
                        .collect();
                    g.secili = eski.and_then(|a| g.liste.iter().position(|r| r.ad == a)).unwrap_or(0);
                    g.on.hazir = true;
                    g.on.kirli = !g.liste.is_empty();
                    if g.liste.is_empty() {
                        g.on.hata = Some(t!("The library has no sky images: add one with \"Add Sky…\".", "Kütüphanede gökyüzü resmi yok: \"Gökyüzü ekle…\" ile ekleyin.").into());
                    }
                }
                Some("hata") => {
                    g.liste_istek = None;
                    g.on.hata = v["mesaj"].as_str().map(str::to_string);
                }
                _ => {}
            }
        } else {
            kalan.push(v);
        }
    }
    // kalan mesajlar önizleme / tam sonuç
    let (tx, rx) = channel();
    for v in kalan {
        let _ = tx.send(v);
    }
    let asil = std::mem::replace(&mut g.on.motor.rx, rx);
    let tam = on_mesajlar(ctx, &mut g.on, "evren-gok-onizleme");
    g.on.motor.rx = asil;
    let tam = match tam {
        Ok(t) => t,
        Err(e) => {
            durum_yaz(app, e, true);
            return Durum::Yok;
        }
    };
    if let Some(yol) = tam {
        if etkinlestir(app, g.on.doc) {
            match app.run(cmds::ADD_LAYER, json!({"path": yol, "name": t!("Sky", "Gökyüzü")})) {
                Ok(_) => durum_yaz(app, t!("Sky: new layer added", "Gökyüzü: yeni katman eklendi"), false),
                Err(e) => durum_yaz(app, e, true),
            }
        }
        HEPSI.with(|h| {
            let mut h = h.borrow_mut();
            h.gok_secili_ad = g.liste.get(g.secili).map(|r| r.ad.clone());
            h.gok_ayar = Some(g.ayar.clone());
        });
        g.on.motor.kapat();
        return Durum::Yok;
    }
    // eklenecek gökyüzü (dosya penceresinden)
    if let Some(yol) = HEPSI.with(|h| h.borrow_mut().gok_eklenecek_al()) {
        g.eklenecek = Some(yol);
    }
    if let Some(yol) = g.eklenecek.take() {
        let _ = g.on.motor.gonder("gok_ekle", json!({"yol": yol}));
        if let Ok(id) = g.on.motor.gonder("gok_liste", json!({})) {
            g.liste_istek = Some(id);
            if let Some(ad) = Path::new(&yol).file_name().and_then(|a| a.to_str()) {
                HEPSI.with(|h| h.borrow_mut().gok_secili_ad = Some(ad.to_string()));
                g.liste.clear();
            }
        }
    }
    let mut acik = true;
    let mut iptal_tik = false;
    let mut uygula = false;
    let mut ekle = false;
    pencere_ac(t!("Replace Sky", "Gökyüzü değiştir"), "evren-gok").open(&mut acik).default_width(720.0).show(ctx, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                onizleme_resmi(ui, g.on.doku.as_ref(), None);
                durum_satiri(ui, &g.on);
            });
            ui.vertical(|ui| {
                ui.set_width(240.0);
                ui.strong(t!("Sky", "Gökyüzü"));
                egui::ScrollArea::vertical().max_height(150.0).id_salt("evren-gok-liste").show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (i, r) in g.liste.iter().enumerate() {
                            let secili = i == g.secili;
                            let resp = match &r.doku {
                                Some(t) => {
                                    let s = t.size_vec2();
                                    let k = 104.0 / s.x.max(1.0);
                                    ui.add(egui::Button::image(egui::Image::new((t.id(), s * k))).selected(secili))
                                }
                                None => ui.selectable_label(secili, &r.ad),
                            };
                            if resp.on_hover_text(&r.ad).clicked() && !secili {
                                g.secili = i;
                                g.on.kirli = true;
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if ui.small_button(t!("Add Sky…", "Gökyüzü ekle…")).clicked() {
                        ekle = true;
                    }
                    if ui.small_button(t!("Open Folder", "Klasörü aç")).clicked()
                        && let Some(k) = kok()
                    {
                        let _ = Command::new("explorer").arg(k.join("gokyuzu_kutuphanesi")).spawn();
                    }
                });
                ui.separator();
                egui::Grid::new("evren-gok-grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    for (i, (ad, _, lo, hi, vars)) in GOK_AYARLARI.iter().enumerate() {
                        ui.label(gok_etiket(ad));
                        let Some(v) = g.ayar.get_mut(i) else { continue };
                        let r = ui.add(egui::Slider::new(v, *lo..=*hi).max_decimals(0));
                        if r.double_clicked() {
                            *v = *vars;
                            g.on.kirli = true;
                        }
                        g.on.kirli |= r.drag_stopped() || (r.changed() && !r.dragged());
                        ui.end_row();
                    }
                });
                if ui.small_button(t!("Reset Settings", "Ayarları sıfırla")).clicked() {
                    g.ayar = GOK_AYARLARI.iter().map(|a| a.4).collect();
                    g.on.kirli = true;
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            let olur = g.on.hazir && g.on.bekleyen.is_none() && !g.liste.is_empty() && g.on.doku.is_some();
            if ui.add_enabled(olur, egui::Button::new(t!("Apply (new layer)", "Uygula (yeni katman)"))).clicked() {
                uygula = true;
            }
            if ui.button(t!("Cancel", "İptal")).clicked() {
                iptal_tik = true;
            }
        });
    });
    acik &= !iptal_tik;
    if ekle {
        let r = app.ask_file(crate::file_dialog::FileDialogRequest::Open { multiple: false, initial_dir: None, extensions: None }, |_app, cevap| {
            if let crate::file_dialog::FileDialogAnswer::Paths(p) = cevap
                && let Some(yol) = p.into_iter().next()
            {
                HEPSI.with(|h| h.borrow_mut().gok_eklenecek = Some(yol));
            }
            Ok(Value::Null)
        });
        if let Err(e) = r {
            durum_yaz(app, e, true);
        }
    }
    if !acik {
        HEPSI.with(|h| {
            let mut h = h.borrow_mut();
            h.gok_secili_ad = g.liste.get(g.secili).map(|r| r.ad.clone());
            h.gok_ayar = Some(g.ayar.clone());
        });
        g.on.motor.kapat();
        return Durum::Yok;
    }
    let ayar: serde_json::Map<String, Value> = GOK_AYARLARI.iter().zip(&g.ayar).map(|(a, v)| (a.1.to_string(), json!(v))).collect();
    let gs = g.on.girdi.to_string_lossy().into_owned();
    if let Some(r) = g.liste.get(g.secili) {
        let gok = r.yol.clone();
        if uygula {
            g.on.asama = t!("Applying at full resolution", "Tam çözünürlükte uygulanıyor").into();
            match g.on.motor.gonder("gok", json!({"girdi": gs, "gok": gok, "cikti": cikti_yolu(g.on.doc, "gok"), "ayar": ayar})) {
                Ok(id) => g.on.bekleyen = Some((id, true)),
                Err(e) => g.on.hata = Some(e),
            }
        } else if g.on.hazir && g.on.kirli && g.on.bekleyen.is_none() && g.liste_istek.is_none() {
            g.on.kirli = false;
            g.on.asama = if g.on.doku.is_none() { t!("Finding the sky", "Gökyüzü aranıyor").into() } else { t!("Preview", "Önizleme").into() };
            match g.on.motor.gonder("gok", json!({"girdi": gs, "gok": gok, "cikti": cikti_yolu(g.on.doc, "gok_on"), "ayar": ayar, "onizleme": 900})) {
                Ok(id) => g.on.bekleyen = Some((id, false)),
                Err(e) => g.on.hata = Some(e),
            }
        }
    }
    Durum::Gok(g)
}

/// Gökyüzü ayarının arayüz adı.
fn gok_etiket(tr: &str) -> &str {
    if super::dil::tr() {
        return tr;
    }
    match tr {
        "Kenarı kaydır" => "Shift edge",
        "Yumuşaklık" => "Feather",
        "Ölçek" => "Scale",
        "Yatay konum" => "Horizontal",
        "Dikey konum" => "Vertical",
        "Parlaklık" => "Brightness",
        "Sıcaklık" => "Temperature",
        "Ön plan uyumu" => "Foreground match",
        "Kenar ışığı" => "Edge light",
        _ => tr,
    }
}

impl Hepsi {
    fn gok_eklenecek_al(&mut self) -> Option<String> {
        self.gok_eklenecek.take()
    }
}
