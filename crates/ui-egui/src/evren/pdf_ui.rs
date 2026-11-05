//! EVREN eklentisi: PDF açma (Photoshop'taki "PDF İçe Aktar" gibi).
//!
//! File › Open, sürükle-bırak, Son Açılanlar ve çift tıklamayla gelen bir .pdf, `file_open.rs`'teki bağlantı
//! satırıyla buraya gelir. Pencerede sayfaların küçük resimleri (birden çok seçilebilir), çözünürlük (ppi),
//! görüntü boyutu ve arka plan (saydam / beyaz) seçilir; seçilen her sayfa ayrı belge olarak açılır.
//!
//! PDF'i resme Windows'un kendi PDF çizicisi çevirir (`evren_ai\pdf_ac.ps1`, Windows.Data.Pdf): ek program yok.

use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use serde_json::Value;

use super::ai_ui;
use crate::PhotocraftApp;
use crate::t;

const EN_BUYUK_KENAR: f64 = 30000.0;
const EN_COK_PIKSEL: f64 = 300_000_000.0;

struct Sayfa {
    no: u32,
    /// Boyut, pt (1/72 inç).
    genislik: f64,
    yukseklik: f64,
    doku: Option<egui::TextureHandle>,
    secili: bool,
}

enum Mesaj {
    Sayfa(u32, f64, f64),
    Cizildi(u32, String),
    Bitti,
    Hata(String),
}

#[derive(PartialEq)]
enum Asama {
    Okunuyor,
    Hazir,
    Aciliyor,
}

struct PdfIce {
    yol: String,
    ad: String,
    klasor: PathBuf,
    sayfalar: Vec<Sayfa>,
    dpi: f64,
    beyaz: bool,
    asama: Asama,
    rx: Receiver<Mesaj>,
    cizilen: Vec<(u32, String)>,
    hata: Option<String>,
}

thread_local! {
    static DURUM: RefCell<Option<Box<PdfIce>>> = const { RefCell::new(None) };
    /// Son seçilen ppi ve arka plan (program açık kaldıkça hatırlanır).
    static SON: RefCell<(f64, bool)> = const { RefCell::new((300.0, false)) };
    /// Açılması istenen PDF (bir sonraki karede pencere açılır).
    static BEKLEYEN: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn betik() -> Result<PathBuf, String> {
    let k = ai_ui::kok().ok_or(t!("evren_ai folder not found (it must be in the PhotoCraft folder)", "evren_ai klasörü bulunamadı (PhotoCraft klasöründe olmalı)"))?;
    let b = k.join("pdf_ac.ps1");
    if b.is_file() { Ok(b) } else { Err(format!("{}: {}", t!("PDF script not found", "PDF betiği bulunamadı"), b.display())) }
}

/// Windows PowerShell 5.1 (WinRT erişimi olan) ile betiği çalıştırır; her çıktı satırı JSON.
fn calistir(ctx: &egui::Context, argumanlar: Vec<String>, cizim: bool) -> Result<Receiver<Mesaj>, String> {
    let b = betik()?;
    let mut c = Command::new("powershell.exe");
    c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(&b).args(&argumanlar);
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut cocuk = c.spawn().map_err(|e| format!("{}: {e}", t!("could not start the PDF reader", "PDF okuyucu açılamadı")))?;
    let cikti = cocuk.stdout.take().ok_or("PDF reader stdout missing")?;
    let hata_akisi = cocuk.stderr.take();
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    let tr = super::dil::tr();
    std::thread::spawn(move || {
        let hata_metni = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut h) = hata_akisi {
                let _ = std::io::Read::read_to_string(&mut h, &mut s);
            }
            s
        });
        for satir in BufReader::new(cikti).lines() {
            let Ok(satir) = satir else { break };
            let Ok(v) = serde_json::from_str::<Value>(satir.trim()) else { continue };
            let no = v["sayfa"].as_u64().unwrap_or(0) as u32;
            let m = if cizim {
                Mesaj::Cizildi(no, v["dosya"].as_str().unwrap_or_default().to_string())
            } else {
                Mesaj::Sayfa(no, v["gen"].as_f64().unwrap_or(0.0), v["yuk"].as_f64().unwrap_or(0.0))
            };
            let _ = tx.send(m);
            ctx.request_repaint();
        }
        let durum = cocuk.wait();
        let hata = hata_metni.join().unwrap_or_default();
        let _ = tx.send(if durum.is_ok_and(|d| d.success()) {
            Mesaj::Bitti
        } else {
            let ilk = hata.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(if tr { "PDF okunamadı" } else { "could not read the PDF" }).to_string();
            Mesaj::Hata(if ilk.to_lowercase().contains("password") || ilk.contains("parola") {
                if tr { "PDF parola korumalı; önce parolayı kaldırın" } else { "the PDF is password protected; remove the password first" }.into()
            } else {
                ilk
            })
        });
        ctx.request_repaint();
    });
    Ok(rx)
}

/// file_open.rs'ten: `path` bir PDF ise içe aktarma penceresini açar ve `Some` döner (belge sonra açılır).
pub fn yakala(app: &mut PhotocraftApp, path: &str) -> Option<Result<Vec<String>, String>> {
    let pdf = std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    if !pdf {
        return None;
    }
    let _ = app;
    // Pencere bir sonraki karede açılır (orada ekran bağlamı var).
    BEKLEYEN.with(|b| *b.borrow_mut() = Some(path.to_string()));
    Some(Ok(Vec::new()))
}

fn ac(ctx: &egui::Context, path: &str) -> Result<(), String> {
    if let Some(eski) = DURUM.with(|d| d.borrow_mut().take()) {
        let _ = std::fs::remove_dir_all(&eski.klasor);
    }
    let kok = ai_ui::kok().ok_or(t!("evren_ai folder not found", "evren_ai klasörü bulunamadı"))?;
    let klasor = kok.join("calisma").join("pdf").join(format!("{}", photocraft_doc::LayerId::fresh().0));
    std::fs::create_dir_all(&klasor).map_err(|e| e.to_string())?;
    let rx = calistir(ctx, vec!["-Pdf".into(), path.into(), "-Cikti".into(), klasor.to_string_lossy().into_owned(), "-Bilgi".into()], false)?;
    let ad = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "PDF".into());
    let (dpi, beyaz) = SON.with(|s| *s.borrow());
    DURUM.with(|d| {
        *d.borrow_mut() = Some(Box::new(PdfIce {
            yol: path.to_string(),
            ad,
            klasor,
            sayfalar: Vec::new(),
            dpi,
            beyaz,
            asama: Asama::Okunuyor,
            rx,
            cizilen: Vec::new(),
            hata: None,
        }))
    });
    Ok(())
}

fn kapat(p: Box<PdfIce>) {
    let _ = std::fs::remove_dir_all(&p.klasor);
}

/// Pencere (her karede, evren::pencereler).
pub fn pencere(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if let Some(yol) = BEKLEYEN.with(|b| b.borrow_mut().take())
        && let Err(e) = ac(ctx, &yol)
    {
        ai_ui::durum_yaz(app, format!("{}: {e}", t!("Could not open the PDF", "PDF açılamadı")), true);
    }
    let Some(mut p) = DURUM.with(|d| d.borrow_mut().take()) else { return };
    let mut bitti = false;
    while let Ok(m) = p.rx.try_recv() {
        match m {
            Mesaj::Sayfa(no, genislik, yukseklik) => {
                let k = p.klasor.join(format!("k_{no}.png"));
                let doku = ai_ui::doku_yukle(ctx, &k.to_string_lossy(), &format!("evren-pdf-{no}"));
                let secili = p.sayfalar.is_empty();
                p.sayfalar.push(Sayfa { no, genislik, yukseklik, doku, secili });
            }
            Mesaj::Cizildi(no, dosya) => p.cizilen.push((no, dosya)),
            Mesaj::Bitti => bitti = true,
            Mesaj::Hata(h) => {
                p.hata = Some(h);
                if p.asama == Asama::Aciliyor {
                    p.asama = Asama::Hazir;
                }
            }
        }
    }
    if bitti {
        match p.asama {
            Asama::Okunuyor => {
                p.asama = Asama::Hazir;
                if p.sayfalar.is_empty() && p.hata.is_none() {
                    p.hata = Some(t!("No pages found in the PDF", "PDF'te sayfa bulunamadı").into());
                }
            }
            Asama::Aciliyor => {
                belgeleri_ac(app, &p);
                SON.with(|s| *s.borrow_mut() = (p.dpi, p.beyaz));
                kapat(p);
                return;
            }
            Asama::Hazir => {}
        }
    }

    let mut acik = true;
    let mut tamam = false;
    let mut iptal = false;
    let baslik = format!("{} — {}", t!("Import PDF", "PDF İçe Aktar"), crate::file_open::display_name(&p.yol));
    egui::Window::new(baslik)
        .id(egui::Id::unique("evren-pdf"))
        .open(&mut acik)
        .collapsible(false)
        .resizable(true)
        .default_size([640.0, 520.0])
        .default_pos(egui::pos2(220.0, 120.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong(format!("{} ({})", t!("Pages", "Sayfalar"), p.sayfalar.len()));
                if p.asama == Asama::Okunuyor {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(t!("None", "Hiçbiri")).clicked() {
                        p.sayfalar.iter_mut().for_each(|s| s.secili = false);
                    }
                    if ui.small_button(t!("Select all", "Tümünü seç")).clicked() {
                        p.sayfalar.iter_mut().for_each(|s| s.secili = true);
                    }
                });
            });
            ui.label(egui::RichText::new(t!("Click the pages to open (Ctrl/Shift+click for several); each page opens as its own document.", "Açmak istediğiniz sayfalara tıklayın (birden çok için Ctrl/Shift); her sayfa ayrı belge olarak açılır.")).small().weak());
            egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for s in &mut p.sayfalar {
                        let kutu = egui::vec2(132.0, 150.0);
                        let (r, resp) = ui.allocate_exact_size(kutu, egui::Sense::click());
                        let gorsel = ui.visuals();
                        let zemin = if s.secili { gorsel.selection.bg_fill } else if resp.hovered() { gorsel.widgets.hovered.bg_fill } else { gorsel.extreme_bg_color };
                        ui.painter().rect_filled(r, 4.0, zemin);
                        let alan = egui::Rect::from_min_size(r.min + egui::vec2(6.0, 6.0), egui::vec2(120.0, 118.0));
                        if let Some(t) = &s.doku {
                            let b = t.size_vec2();
                            let k = (alan.width() / b.x).min(alan.height() / b.y);
                            let ir = egui::Rect::from_center_size(alan.center(), b * k);
                            ui.painter().image(t.id(), ir, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                            ui.painter().rect_stroke(ir, 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(90)), egui::StrokeKind::Outside);
                        }
                        ui.painter().text(
                            egui::pos2(r.center().x, r.max.y - 13.0),
                            egui::Align2::CENTER_CENTER,
                            format!("{}", s.no),
                            egui::FontId::proportional(13.0),
                            if s.secili { gorsel.selection.stroke.color } else { gorsel.text_color() },
                        );
                        if resp.clicked() {
                            if ui.input(|i| i.modifiers.command || i.modifiers.shift) {
                                // Ctrl / Shift + tık: çoklu seçim.
                                s.secili = !s.secili;
                            } else {
                                // Tek tık: yalnız bu sayfa (aşağıda uygulanır).
                                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("evren-pdf-tek"), s.no));
                            }
                        }
                    }
                });
            });
            // Tek tıkla seçilen sayfa dışındakiler bırakılır (Photoshop'taki gibi).
            if let Some(no) = ui.ctx().data_mut(|d| d.remove_temp::<u32>(egui::Id::unique("evren-pdf-tek"))) {
                p.sayfalar.iter_mut().for_each(|s| s.secili = s.no == no);
            }
            ui.separator();
            let secili: Vec<&Sayfa> = p.sayfalar.iter().filter(|s| s.secili).collect();
            egui::Grid::new("evren-pdf-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(t!("Resolution", "Çözünürlük"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut p.dpi).range(36.0..=1200.0).speed(1.0).max_decimals(0).suffix(" ppi"));
                    for d in [150.0, 300.0, 600.0] {
                        if ui.selectable_label((p.dpi - d).abs() < 0.5, format!("{d:.0}")).clicked() {
                            p.dpi = d;
                        }
                    }
                });
                ui.end_row();
                ui.label(t!("Image size", "Görüntü boyutu"));
                match secili.first() {
                    Some(s) => {
                        let (w, h) = (s.genislik / 72.0 * p.dpi, s.yukseklik / 72.0 * p.dpi);
                        let cm = |pt: f64| pt / 72.0 * 2.54;
                        let ek = if secili.len() > 1 { t!(format!("  (page {}; {} pages selected)", s.no, secili.len()), format!("  (sayfa {}; {} sayfa seçili)", s.no, secili.len())) } else { String::new() };
                        ui.label(t!(format!("{:.0} × {:.0} pixels  ·  {:.1} × {:.1} cm{ek}", w.round(), h.round(), cm(s.genislik), cm(s.yukseklik)), format!("{:.0} × {:.0} piksel  ·  {:.1} × {:.1} cm{ek}", w.round(), h.round(), cm(s.genislik), cm(s.yukseklik)).replace('.', ",")));
                    }
                    None => {
                        ui.label(egui::RichText::new(t!("No page selected", "Sayfa seçilmedi")).weak());
                    }
                }
                ui.end_row();
                ui.label(t!("Background", "Arka plan"));
                ui.horizontal(|ui| {
                    if ui.selectable_label(!p.beyaz, t!("Transparent", "Saydam")).clicked() {
                        p.beyaz = false;
                    }
                    if ui.selectable_label(p.beyaz, t!("White", "Beyaz")).clicked() {
                        p.beyaz = true;
                    }
                });
                ui.end_row();
                ui.label(t!("Mode", "Mod"));
                ui.label(t!("RGB Color · 8 bit", "RGB Renk · 8 bit"));
                ui.end_row();
            });
            let buyuk = secili.iter().any(|s| {
                let (w, h) = (s.genislik / 72.0 * p.dpi, s.yukseklik / 72.0 * p.dpi);
                w.max(h) > EN_BUYUK_KENAR || w * h > EN_COK_PIKSEL
            });
            if buyuk {
                ui.colored_label(ui.visuals().error_fg_color, t!("The page is too large at this resolution; lower the ppi.", "Bu çözünürlükte sayfa çok büyük; ppi değerini düşürün."));
            }
            if let Some(h) = &p.hata {
                ui.colored_label(ui.visuals().error_fg_color, h);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if p.asama == Asama::Aciliyor {
                    ui.spinner();
                    ui.label(format!("{} ({}/{})", t!("Rendering pages", "Sayfalar çiziliyor"), p.cizilen.len(), secili.len()));
                } else {
                    let olur = p.asama == Asama::Hazir && !secili.is_empty() && !buyuk;
                    if ui.add_enabled(olur, egui::Button::new(t!("OK", "Tamam"))).clicked() {
                        tamam = true;
                    }
                }
                if ui.button(t!("Cancel", "İptal")).clicked() {
                    iptal = true;
                }
            });
        });
    if iptal || !acik {
        kapat(p);
        return;
    }
    if tamam {
        let sayfalar: Vec<String> = p.sayfalar.iter().filter(|s| s.secili).map(|s| s.no.to_string()).collect();
        let mut arg = vec![
            "-Pdf".into(),
            p.yol.clone(),
            "-Cikti".into(),
            p.klasor.to_string_lossy().into_owned(),
            "-Sayfalar".into(),
            sayfalar.join(","),
            "-Dpi".into(),
            format!("{:.0}", p.dpi),
        ];
        if p.beyaz {
            arg.push("-Beyaz".into());
        }
        match calistir(ctx, arg, true) {
            Ok(rx) => {
                p.rx = rx;
                p.cizilen.clear();
                p.hata = None;
                p.asama = Asama::Aciliyor;
            }
            Err(e) => p.hata = Some(e),
        }
    }
    DURUM.with(|d| *d.borrow_mut() = Some(p));
}

/// Çizilen sayfaları ayrı belgeler olarak açar (dosyasız: Kaydet, Farklı Kaydet gibi yer sorar).
fn belgeleri_ac(app: &mut PhotocraftApp, p: &PdfIce) {
    let tek = p.cizilen.len() == 1 && p.sayfalar.len() == 1;
    let mut acilan = 0;
    for (no, dosya) in &p.cizilen {
        let yol = p.klasor.join(dosya);
        let ad = if tek { p.ad.clone() } else { t!(format!("{} - Page {no}", p.ad), format!("{} - Sayfa {no}", p.ad)) };
        match std::fs::read(&yol).map_err(|e| e.to_string()).and_then(|b| app.open_bytes_from(&ad, &b, None)) {
            Ok(_) => acilan += 1,
            Err(e) => ai_ui::durum_yaz(app, t!(format!("Could not open page {no}: {e}"), format!("Sayfa {no} açılamadı: {e}")), true),
        }
    }
    if acilan > 0 {
        ai_ui::durum_yaz(app, t!(format!("{}: {acilan} page(s) opened ({:.0} ppi)", crate::file_open::display_name(&p.yol), p.dpi), format!("{}: {acilan} sayfa açıldı ({:.0} ppi)", crate::file_open::display_name(&p.yol), p.dpi)), false);
    }
}
