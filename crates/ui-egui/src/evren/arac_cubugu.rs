//! EVREN eklentisi: sol araç çubuğunun en altındaki kendi düğmelerimiz (simgeleri PhotoCraft'ın simgeleriyle
//! aynı biçimde: beyaz SVG, tema rengine boyanır). Varsayılan sıra: 360 · Upscale · Alan derinliği · Bitki · Cam ·
//! Gökyüzü · Gürültü temizle · İnsanları düzelt · Çizgi. Araç çubuğu çift sütunsa düğmeler de çift sütun dizilir. Düğmeler fareyle tutulup sürüklenerek
//! yeniden sıralanır; sıra `evren_ai\ayarlar.json` dosyasında saklanır.

use std::cell::RefCell;
use std::sync::OnceLock;

use craft_ui::icons::{SvgIconSet, SvgStyle};
use egui::{Color32, Rect, Sense, Vec2, vec2};

use super::ai_ui::{self, Arac};
use super::pano_ui;
use crate::PhotocraftApp;

static SIMGE_DOSYALARI: &[(&str, &[u8])] = &[
    ("pano", include_bytes!("icons/pano.svg")),
    ("upscale", include_bytes!("icons/upscale.svg")),
    ("dof", include_bytes!("icons/dof.svg")),
    ("bitki", include_bytes!("icons/bitki.svg")),
    ("cam", include_bytes!("icons/cam.svg")),
    ("gok", include_bytes!("icons/gok.svg")),
    ("gurultu", include_bytes!("icons/gurultu.svg")),
    ("insan", include_bytes!("icons/insan.svg")),
    ("cizgi", include_bytes!("icons/cizgi.svg")),
    ("ok", include_bytes!("icons/ok.svg")),
];

/// Düğmeler: kimlik ve üzerine gelince çıkan açıklama (İngilizce başlık, açıklama; Türkçe başlık, açıklama).
struct Dugme {
    id: &'static str,
    en: (&'static str, &'static str),
    tr: (&'static str, &'static str),
}

const DUGMELER: &[Dugme] = &[
    Dugme {
        id: "pano",
        en: ("360 Settings", "Turn and zoom a 360 layer, add a map pin, export the whole sphere. Make a layer 360 by right-clicking it in Layers › Make 360."),
        tr: ("360 Ayarları", "360 katmanını çevirin ve yakınlaştırın, harita pini ekleyin, kürenin tamamını dışa aktarın. Bir katmanı 360 yapmak için Katmanlar'da sağ tık › 360 Derece Yap."),
    },
    Dugme {
        id: "upscale",
        en: ("Upscale (AI)", "Enlarge the image 1–8× with SUPIR (adds real detail) or 4x-UltraSharp (fast). Can run on every open tab. The canvas grows; the result is a new layer."),
        tr: ("Upscale (yapay zekâ)", "Resmi 1–8 kat büyütür: SUPIR (gerçek detay ekler) ya da 4x-UltraSharp (hızlı). Bütün açık sekmelere de uygulanabilir. Tuval büyür, sonuç yeni katmandır."),
    },
    Dugme {
        id: "dof",
        en: ("Depth of Field", "Blur the background or foreground like a real lens. Click the spot that should stay sharp; AI estimates the depth."),
        tr: ("Alan derinliği", "Arka planı ya da ön planı gerçek bir objektif gibi bulanıklaştırır. Net kalacak yere tıklayın; derinliği yapay zekâ hesaplar."),
    },
    Dugme {
        id: "bitki",
        en: ("Separate Plants", "Finds trees, grass and plants and copies them to a new transparent layer."),
        tr: ("Bitkileri ayır", "Ağaçları, çimleri ve bitkileri bulup saydam yeni bir katmana kopyalar."),
    },
    Dugme {
        id: "cam",
        en: ("Separate Glass", "Finds windows and glass and copies them to a new transparent layer, ready for reflections."),
        tr: ("Camları ayır", "Pencereleri ve camları bulup saydam yeni bir katmana kopyalar; yansıma eklemeye hazır."),
    },
    Dugme {
        id: "gok",
        en: ("Replace Sky", "Finds the sky and replaces it with one from your library, with live preview, edge and color matching."),
        tr: ("Gökyüzü değiştir", "Gökyüzünü bulup kütüphanenizdeki bir gökyüzüyle değiştirir; canlı önizleme, kenar ve renk uyumu."),
    },
    Dugme {
        id: "gurultu",
        en: ("Denoise", "Removes render noise from the selected layer on the graphics card (Intel Open Image Denoise, like Corona). Live amount and sharpness."),
        tr: ("Gürültü temizle", "Seçili katmandaki render gürültüsünü ekran kartında temizler (Intel Open Image Denoise, Corona'daki gibi). Miktar ve keskinlik canlı."),
    },
    Dugme {
        id: "insan",
        en: ("Fix People", "Redraws distorted people and faces in renders and upscales. Select an area first to fix only there. The result is a new layer."),
        tr: ("İnsanları düzelt", "Render ve upscale'lerde bozuk çıkan insanları ve yüzleri yeniden çizer. Önce bir alan seçerseniz yalnız orayı düzeltir. Sonuç yeni katmandır."),
    },
    Dugme {
        id: "cizgi",
        en: ("Line / Arrow", "Draw freely, then hold without releasing: the stroke snaps into a clean line, circle, rectangle, polygon or curve. Thickness = brush size."),
        tr: ("Çizgi / Ok", "Serbest çizin, sonra bırakmadan bekleyin: çizim pürüzsüz çizgiye, daireye, dikdörtgene, çokgene ya da eğriye dönüşür. Kalınlık = fırça boyutu."),
    },
];

/// Düğmenin başlığı ve açıklaması (arayüz diline göre).
fn aciklama(id: &str) -> (&'static str, &'static str) {
    DUGMELER.iter().find(|d| d.id == id).map_or(("", ""), |d| if super::dil::tr() { d.tr } else { d.en })
}

fn simgeler() -> &'static SvgIconSet {
    static SET: OnceLock<SvgIconSet> = OnceLock::new();
    SET.get_or_init(|| SvgIconSet::new(SIMGE_DOSYALARI, SvgStyle { current_color: "#ffffff", stroke_width: Some("1.75") }, "bytes://evren-icons/", "pano"))
}

/// Simge resmi (boyanmış).
pub fn simge(ad: &str, boy: f32, renk: Color32) -> egui::Image<'static> {
    egui::Image::new(simgeler().source(ad)).fit_to_exact_size(Vec2::splat(boy)).tint(renk)
}

// ------------------------------------------------------------------ sıra (kalıcı)

thread_local! {
    static SIRA: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

fn ayar_dosyasi() -> Option<std::path::PathBuf> {
    ai_ui::kok().map(|k| k.join("ayarlar.json"))
}

/// Geçerli sıra: dosyadaki sıra, bilinmeyenler atılır, eksikler sona eklenir.
fn sira() -> Vec<String> {
    SIRA.with(|s| {
        let mut s = s.borrow_mut();
        if s.is_none() {
            let kayitli: Vec<String> = ayar_dosyasi()
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .and_then(|v| v["dugme_sirasi"].as_array().cloned())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let mut v: Vec<String> = kayitli.into_iter().filter(|id| DUGMELER.iter().any(|d| d.id == id)).collect();
            v.dedup();
            for Dugme { id, .. } in DUGMELER {
                if !v.iter().any(|x| x == id) {
                    v.push((*id).to_string());
                }
            }
            *s = Some(v);
        }
        s.clone().unwrap_or_default()
    })
}

fn sira_kaydet(v: Vec<String>) {
    if let Some(p) = ayar_dosyasi() {
        let mut j = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).filter(|j| j.is_object()).unwrap_or_else(|| serde_json::json!({}));
        j["dugme_sirasi"] = serde_json::json!(v);
        let _ = std::fs::write(&p, serde_json::to_vec_pretty(&j).unwrap_or_default());
    }
    SIRA.with(|s| *s.borrow_mut() = Some(v));
}

// ------------------------------------------------------------------ düzen

const ARALIK: f32 = 4.0;

fn sutun(cift: bool) -> usize {
    if cift { 2 } else { 1 }
}

/// En alttaki düğmelerin kapladığı yükseklik (araç listesi bu kadar kısalır).
pub fn yukseklik(kutu: f32, cift: bool) -> f32 {
    let satir = DUGMELER.len().div_ceil(sutun(cift)) as f32;
    satir * kutu + (satir - 1.0) * ARALIK + 22.0
}

fn acik_mi(app: &PhotocraftApp, ctx: &egui::Context, id: &str) -> bool {
    match id {
        "pano" => pano_ui::window_open(ctx),
        "upscale" => ai_ui::acik(Arac::Upscale),
        "dof" => ai_ui::acik(Arac::Dof),
        "bitki" => ai_ui::acik(Arac::Bitki),
        "cam" => ai_ui::acik(Arac::Cam),
        "gok" => ai_ui::acik(Arac::Gok),
        "gurultu" => super::gurultu_ui::acik(),
        "insan" => ai_ui::acik(Arac::Insan),
        "cizgi" => super::cizgi_ui::acik(app),
        _ => false,
    }
}

fn calistir(app: &mut PhotocraftApp, ctx: &egui::Context, id: &str) {
    let arac = match id {
        "pano" => {
            if pano_ui::window_open(ctx) {
                pano_ui::close_window(ctx);
            } else {
                pano_ui::open_window(ctx);
            }
            return;
        }
        "gurultu" => {
            super::gurultu_ui::ac(app, ctx);
            return;
        }
        "upscale" => Arac::Upscale,
        "dof" => Arac::Dof,
        "bitki" => Arac::Bitki,
        "cam" => Arac::Cam,
        "gok" => Arac::Gok,
        "insan" => Arac::Insan,
        _ => return,
    };
    ai_ui::ac(app, ctx, arac);
}

/// Düğmeler (panels/toolbar.rs'ten).
pub fn dugmeler(app: &mut PhotocraftApp, ui: &mut egui::Ui, kutu: f32) {
    let ctx = ui.ctx().clone();
    let cift = app.ui.panels.toolbar_double;
    let n_sutun = sutun(cift);
    let sira = sira();
    let n = sira.len();
    let satir = n.div_ceil(n_sutun);
    // Sütun aralığı araç çubuğunun genişliğine göre (araçların ızgarasıyla hizalı).
    let ara_x = if n_sutun > 1 { ((ui.available_width() - n_sutun as f32 * kutu) / (n_sutun as f32 - 1.0)).clamp(0.0, 8.0) } else { 0.0 };
    let genislik = n_sutun as f32 * kutu + (n_sutun as f32 - 1.0) * ara_x;
    ui.add_space(4.0);
    let (cizgi, _) = ui.allocate_exact_size(vec2(genislik, 1.0), Sense::hover());
    ui.painter().hline(cizgi.x_range(), cizgi.center().y, ui.visuals().widgets.noninteractive.bg_stroke);
    ui.add_space(4.0);
    let (alan, _) = ui.allocate_exact_size(vec2(genislik, satir as f32 * kutu + (satir as f32 - 1.0) * ARALIK), Sense::hover());
    let yuva = |i: usize| -> Rect {
        let (sat, sut) = (i / n_sutun, i % n_sutun);
        Rect::from_min_size(alan.min + vec2(sut as f32 * (kutu + ara_x), sat as f32 * (kutu + ARALIK)), Vec2::splat(kutu))
    };
    let surukle_id = egui::Id::unique("evren-arac-surukle");
    let surukleniyor: Option<usize> = ctx.data(|d| d.get_temp::<usize>(surukle_id)).filter(|i| *i < n);
    let isaretci = ctx.pointer_interact_pos();
    // Bırakılacak yuva: işaretçiye en yakın yuva.
    let hedef = surukleniyor.and(isaretci).map(|p| (0..n).min_by(|a, b| yuva(*a).center().distance(p).total_cmp(&yuva(*b).center().distance(p))).unwrap_or(0));
    let boy = (kutu * 0.56).round();
    let mut tiklanan = None;
    let mut birakildi = None;
    for (i, id) in sira.iter().enumerate() {
        let r = yuva(i);
        let (baslik, ipucu) = aciklama(id);
        let resp = ui.interact(r, egui::Id::unique(("evren-arac", id.as_str())), Sense::click_and_drag());
        if surukleniyor == Some(i) {
            // Sürüklenen düğmenin yeri soluk kalır.
            ui.painter().rect_stroke(r.shrink(1.0), 4.0, egui::Stroke::new(1.0, ui.visuals().weak_text_color()), egui::StrokeKind::Inside);
        } else {
            let renk = crate::icons::button_chrome(ui, r, acik_mi(app, &ctx, id), resp.hovered() && surukleniyor.is_none());
            simge(id, boy, renk).paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(boy)));
        }
        // Sürükleme elle izlenir (araç çubuğunun kendi sürüklemesi egui'nin sürüklemesini üstleniyor):
        // düğmede basılı tutup 6 pikselden fazla kaydırınca başlar.
        if surukleniyor.is_none()
            && resp.is_pointer_button_down_on()
            && let (Some(o), Some(p)) = (ctx.input(|inp| inp.pointer.press_origin()), isaretci)
            && o.distance(p) > 6.0
        {
            ctx.data_mut(|d| d.insert_temp(surukle_id, i));
        }
        if surukleniyor.is_none() {
            let resp = resp.on_hover_ui(|ui| {
                ui.set_max_width(260.0);
                ui.label(egui::RichText::new(baslik).strong());
                ui.label(ipucu);
            });
            if resp.clicked() {
                tiklanan = Some((id.clone(), r));
            }
        }
    }
    if let (Some(kaynak), Some(p)) = (surukleniyor, isaretci) {
        if let Some(h) = hedef {
            ui.painter().rect_stroke(yuva(h), 4.0, egui::Stroke::new(1.5, ui.visuals().selection.stroke.color), egui::StrokeKind::Inside);
        }
        // Sürüklenen simge işaretçiyle gider (her şeyin üstünde).
        if let Some(id) = sira.get(kaynak) {
            let katman = egui::LayerId::new(egui::Order::Tooltip, surukle_id);
            let r = Rect::from_center_size(p, Vec2::splat(kutu));
            let painter = ctx.layer_painter(katman);
            painter.rect_filled(r, 4.0, ui.visuals().widgets.hovered.bg_fill);
            simge(id, boy, ui.visuals().strong_text_color()).paint_at(&ui.new_child(egui::UiBuilder::new().layer_id(katman).max_rect(r)), Rect::from_center_size(p, Vec2::splat(boy)));
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if let Some(k) = surukleniyor
        && !ctx.input(|inp| inp.pointer.primary_down())
    {
        birakildi = Some(k);
    }
    if surukleniyor.is_some() {
        ctx.request_repaint();
    }
    if let Some(kaynak) = birakildi {
        ctx.data_mut(|d| d.remove::<usize>(surukle_id));
        if let Some(h) = hedef.filter(|h| *h != kaynak) {
            let mut yeni = sira.clone();
            let id = yeni.remove(kaynak);
            yeni.insert(h.min(yeni.len()), id);
            sira_kaydet(yeni);
        }
    }
    if let Some((id, r)) = tiklanan {
        if id == "cizgi" {
            super::cizgi_ui::dugme(&ctx, r);
        } else {
            calistir(app, &ctx, &id);
        }
    }
}
