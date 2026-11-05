//! EVREN eklentisi: Çizgi aracı (Procreate'teki QuickShape gibi).
//!
//! Sol alttaki "Çizgi" düğmesi iki seçenek açar: **Çizgi** ve **Ok** (çizginin sonunda ok ucu). Seçilince
//! PhotoCraft'ın Fırça aracı seçilir: kalınlık fırça boyutudur (sağ tık → boyut, boyut halkası), renk ön plan
//! rengidir, opaklık fırçanınkidir. Sol tıkla serbest çizilir; fareyi BIRAKMADAN ~yarım saniye beklenirse çizim
//! tanınıp pürüzsüzleşir: düz / eğik çizgi (yataya, dikeye yakınsa tam düz), daire / elips, kare / dikdörtgen,
//! üçgen ve çokgenler, köşeli çizgiler, yay ve yumuşak eğriler. Düzleşmiş çizginin ucu fareyle sürüklenebilir.
//! Bırakınca seçili katmana pürüzsüz kenarla çizilir (tek geri alma adımı; `evren.cizgi.ciz`).
//! Başka bir araç seçilince Çizgi modu kapanır.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use egui::{Color32, Pos2, Rect, Stroke, pos2, vec2};
use photocraft_engine::evren_ai_cmds as cmds;
use rayon::prelude::*;
use serde_json::json;

use crate::canvas::{ToolEvent, ViewXform};
use crate::{PhotocraftApp, Tool, t};

type P = [f64; 2];

/// Bu kadar bekleyince şekil tanınır.
const BEKLEME: Duration = Duration::from_millis(450);
/// Bekleme sayılması için imlecin oynayabileceği en fazla mesafe (ekran pikseli).
const KIPIRDAMA: f64 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tur {
    Cizgi,
    Ok,
}

#[derive(Clone)]
enum Sekil {
    Cizgi(P, P),
    /// Köşeli çizgi ya da çokgen (kapalı).
    Coklu(Vec<P>, bool),
    Elips { m: P, rx: f64, ry: f64, aci: f64 },
    /// Yay ya da yumuşak eğri (sık örneklenmiş).
    Egri(Vec<P>),
}

struct Cizim {
    noktalar: Vec<P>,
    sekil: Option<Sekil>,
    /// Bekleme ölçümü: son "kıpırdama" yeri ve zamanı.
    capa: P,
    capa_zaman: Instant,
}

#[derive(Default)]
struct Durum {
    tur: Option<Tur>,
    cizim: Option<Cizim>,
    /// Seçenek menüsü açık: düğmenin kutusu.
    menu: Option<Rect>,
    menu_acildi: Option<u64>,
}

thread_local! {
    static DURUM: RefCell<Durum> = RefCell::new(Durum::default());
}

/// Çizgi modu açık mı (düğme seçili görünür).
pub fn acik(app: &PhotocraftApp) -> bool {
    app.ui.tool == Tool::Brush && DURUM.with(|d| d.borrow().tur.is_some())
}

/// Sol alttaki düğmeye tıklandı: seçenek menüsü düğmenin yanında açılır.
pub fn dugme(ctx: &egui::Context, kutu: Rect) {
    let kare = ctx.cumulative_pass_nr();
    DURUM.with(|d| {
        let mut d = d.borrow_mut();
        if d.menu.is_some() {
            d.menu = None;
        } else {
            d.menu = Some(kutu);
            d.menu_acildi = Some(kare);
        }
    });
}

fn sec(app: &mut PhotocraftApp, tur: Option<Tur>) {
    DURUM.with(|d| {
        let mut d = d.borrow_mut();
        d.tur = tur;
        d.cizim = None;
        d.menu = None;
    });
    if tur.is_some() {
        app.ui.tool = Tool::Brush;
        app.ui.status = match tur {
            Some(Tur::Ok) => t!("Arrow: draw, then hold without releasing to snap it clean. Thickness: right-click (brush size)", "Ok: çizin; bırakmadan bekleyince düzleşir. Kalınlık: sağ tık (fırça boyutu)").into(),
            _ => t!("Line: draw, then hold without releasing to snap it clean. Thickness: right-click (brush size)", "Çizgi: çizin; bırakmadan bekleyince düzleşir. Kalınlık: sağ tık (fırça boyutu)").into(),
        };
        app.ui.status_error = false;
    }
}

// ------------------------------------------------------------------ fare (canvas/tool_input.rs'ten)

/// Tuval olayları: Çizgi modu açıkken Fırça aracının sol tık çizimini üstlenir.
pub fn pointer(app: &mut PhotocraftApp, tool: Tool, ev: ToolEvent, mods: egui::Modifiers) -> bool {
    let aktif = tool == Tool::Brush && DURUM.with(|d| d.borrow().tur.is_some()) && !mods.alt && !mods.command;
    let ciziyor = DURUM.with(|d| d.borrow().cizim.is_some());
    if !aktif && !ciziyor {
        return false;
    }
    let zoom = ViewXform::active(app).map_or(1.0, |x| f64::from(x.zoom)).max(1e-6);
    match ev {
        ToolEvent::Down { x, y, .. } => {
            DURUM.with(|d| {
                d.borrow_mut().cizim = Some(Cizim { noktalar: vec![[x, y]], sekil: None, capa: [x, y], capa_zaman: Instant::now() });
            });
            true
        }
        ToolEvent::Move { x, y, .. } => {
            DURUM.with(|d| {
                let mut d = d.borrow_mut();
                let Some(c) = d.cizim.as_mut() else { return };
                match &mut c.sekil {
                    // Düzleşmiş çizginin ucu imleci izler (Procreate gibi).
                    Some(Sekil::Cizgi(_, b)) => *b = [x, y],
                    Some(_) => {}
                    None => {
                        if c.noktalar.last().is_none_or(|l| uzak(*l, [x, y]) * zoom >= 0.75) {
                            c.noktalar.push([x, y]);
                        }
                        if uzak(c.capa, [x, y]) * zoom > KIPIRDAMA {
                            c.capa = [x, y];
                            c.capa_zaman = Instant::now();
                        }
                    }
                }
            });
            true
        }
        ToolEvent::Up { x, y } => {
            let c = DURUM.with(|d| d.borrow_mut().cizim.take());
            if let Some(mut c) = c {
                if c.sekil.is_none() && c.noktalar.last().is_none_or(|l| *l != [x, y]) {
                    c.noktalar.push([x, y]);
                }
                bitir(app, c);
            }
            true
        }
    }
}

/// Bırakınca: şekil (ya da hafifçe yumuşatılmış serbest çizim) katmana çizilir.
fn bitir(app: &mut PhotocraftApp, c: Cizim) {
    let tur = DURUM.with(|d| d.borrow().tur).unwrap_or(Tur::Cizgi);
    let w = f64::from(app.session.tools.brush.size).max(0.5);
    let (yol, kapali) = match &c.sekil {
        Some(s) => sekil_yolu(s),
        None => (yumusat(&c.noktalar), false),
    };
    let Some(st) = app.session.active() else { return };
    let (dw, dh) = (st.doc.size.width as i32, st.doc.size.height as i32);
    let Some((bolge, kapsam)) = rasterle(&yol, kapali, w, tur == Tur::Ok && !kapali, dw, dh) else { return };
    let fg = app.session.tools.foreground;
    let opaklik = app.session.tools.brush.opacity.clamp(0.0, 1.0);
    cmds::cizgi_birak(bolge, kapsam);
    if let Err(e) = app.run(cmds::CIZGI, json!({"renk": [fg[0], fg[1], fg[2]], "opaklik": opaklik})) {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}

// ------------------------------------------------------------------ her kare: bekleme, önizleme, menü

/// Her karede (evren::pencereler).
pub fn pencere(app: &mut PhotocraftApp, ctx: &egui::Context) {
    // Başka araç seçildiyse Çizgi modu kapanır.
    if app.ui.tool != Tool::Brush && DURUM.with(|d| d.borrow().tur.is_some()) {
        DURUM.with(|d| {
            let mut d = d.borrow_mut();
            d.tur = None;
            d.cizim = None;
        });
    }
    menu(app, ctx);
    let zoom = ViewXform::active(app).map_or(1.0, |x| f64::from(x.zoom)).max(1e-6);
    // Bırakmadan beklendi: şekli tanı.
    let ciziyor = DURUM.with(|d| {
        let mut d = d.borrow_mut();
        let Some(c) = d.cizim.as_mut() else { return false };
        if c.sekil.is_none() && c.capa_zaman.elapsed() >= BEKLEME && c.noktalar.len() >= 2 {
            c.sekil = Some(tani(&c.noktalar, zoom));
        }
        true
    });
    if !ciziyor {
        return;
    }
    ctx.request_repaint_after(Duration::from_millis(40));
    onizle(app, ctx);
}

fn onizle(app: &PhotocraftApp, ctx: &egui::Context) {
    let Some(xf) = ViewXform::active(app) else { return };
    let (yol, kapali, tur) = DURUM.with(|d| {
        let d = d.borrow();
        let tur = d.tur.unwrap_or(Tur::Cizgi);
        match d.cizim.as_ref() {
            Some(c) => match &c.sekil {
                Some(s) => {
                    let (y, k) = sekil_yolu(s);
                    (y, k, tur)
                }
                None => (c.noktalar.clone(), false, tur),
            },
            None => (Vec::new(), false, tur),
        }
    });
    if yol.is_empty() {
        return;
    }
    let w = f64::from(app.session.tools.brush.size).max(0.5);
    let fg = app.session.tools.foreground;
    let renk = Color32::from_rgb((fg[0].clamp(0.0, 1.0) * 255.0) as u8, (fg[1].clamp(0.0, 1.0) * 255.0) as u8, (fg[2].clamp(0.0, 1.0) * 255.0) as u8);
    let ok = tur == Tur::Ok && !kapali;
    let (govde, bas) = ok_parcala(&yol, w, ok);
    let ekran = |p: &P| xf.to_screen(p[0] as f32, p[1] as f32);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("evren-cizgi-onizleme"))).with_clip_rect(xf.rect);
    let kalin = (w as f32 * xf.zoom).max(1.0);
    let mut pts: Vec<Pos2> = govde.iter().map(ekran).collect();
    if kapali && let Some(f) = pts.first().copied() {
        pts.push(f);
    }
    if pts.len() == 1 {
        painter.circle_filled(pts[0], kalin / 2.0, renk);
    } else {
        for p in &pts {
            painter.circle_filled(*p, kalin / 2.0, renk);
        }
        painter.add(egui::Shape::line(pts, Stroke::new(kalin, renk)));
    }
    if let Some(u) = bas {
        painter.add(egui::Shape::convex_polygon(u.iter().map(ekran).collect(), renk, Stroke::NONE));
    }
}

fn menu(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let Some(kutu) = DURUM.with(|d| d.borrow().menu) else { return };
    let tur = DURUM.with(|d| d.borrow().tur);
    let mut secilen: Option<Option<Tur>> = None;
    let cevap = egui::Area::new(egui::Id::unique("evren-cizgi-menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(kutu.right_top() + vec2(6.0, 0.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(150.0);
                for (t, ad, simge) in [(Tur::Cizgi, t!("Line", "Çizgi"), "cizgi"), (Tur::Ok, t!("Arrow", "Ok"), "ok")] {
                    let sec_mi = acik(app) && tur == Some(t);
                    let (r, resp) = ui.allocate_exact_size(vec2(150.0, 30.0), egui::Sense::click());
                    if sec_mi || resp.hovered() {
                        ui.painter().rect_filled(r, 4.0, if sec_mi { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.hovered.bg_fill });
                    }
                    let renk = if sec_mi { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
                    super::arac_cubugu::simge(simge, 18.0, renk).paint_at(ui, Rect::from_center_size(pos2(r.min.x + 18.0, r.center().y), vec2(18.0, 18.0)));
                    ui.painter().text(pos2(r.min.x + 36.0, r.center().y), egui::Align2::LEFT_CENTER, ad, egui::FontId::proportional(14.0), renk);
                    if resp.clicked() {
                        secilen = Some(Some(t));
                    }
                }
                if acik(app) {
                    ui.separator();
                    if ui.add_sized([150.0, 24.0], egui::Button::new(t!("Turn off Line", "Çizgiyi kapat"))).clicked() {
                        secilen = Some(None);
                    }
                }
            });
        });
    if let Some(s) = secilen {
        sec(app, s);
        return;
    }
    // Menünün dışına tıklanınca kapanır (açıldığı kare hariç).
    let acildi = DURUM.with(|d| d.borrow().menu_acildi);
    if acildi != Some(ctx.cumulative_pass_nr())
        && ctx.input(|i| i.pointer.any_pressed())
        && !ctx.pointer_interact_pos().is_some_and(|p| cevap.response.rect.contains(p) || kutu.contains(p))
    {
        DURUM.with(|d| d.borrow_mut().menu = None);
    }
}

// ------------------------------------------------------------------ geometri

fn uzak(a: P, b: P) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn uzunluk(p: &[P]) -> f64 {
    p.windows(2).map(|w| uzak(w[0], w[1])).sum()
}

/// p'nin [a, b] parçasına uzaklığı.
fn parca_uzakligi(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    if l2 <= 1e-12 {
        return uzak(p, a);
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
    uzak(p, [a[0] + t * dx, a[1] + t * dy])
}

/// Ramer–Douglas–Peucker sadeleştirme.
fn rdp(p: &[P], eps: f64) -> Vec<P> {
    if p.len() < 3 {
        return p.to_vec();
    }
    let (a, b) = (p[0], p[p.len() - 1]);
    let (mut en, mut i_en) = (0.0, 0);
    for (i, q) in p.iter().enumerate().take(p.len() - 1).skip(1) {
        let d = parca_uzakligi(*q, a, b);
        if d > en {
            en = d;
            i_en = i;
        }
    }
    if en > eps {
        let mut sol = rdp(&p[..=i_en], eps);
        let sag = rdp(&p[i_en..], eps);
        sol.pop();
        sol.extend(sag);
        sol
    } else {
        vec![a, b]
    }
}

/// Yol boyunca eşit aralıklı n nokta.
fn yeniden_ornekle(p: &[P], n: usize) -> Vec<P> {
    let l = uzunluk(p);
    if p.len() < 2 || l <= 0.0 {
        return p.to_vec();
    }
    let adim = l / (n - 1) as f64;
    let mut out = vec![p[0]];
    let (mut i, mut kalan, mut onceki) = (1, adim, p[0]);
    while i < p.len() && out.len() < n {
        let d = uzak(onceki, p[i]);
        if d >= kalan {
            let t = kalan / d;
            let q = [onceki[0] + (p[i][0] - onceki[0]) * t, onceki[1] + (p[i][1] - onceki[1]) * t];
            out.push(q);
            onceki = q;
            kalan = adim;
        } else {
            kalan -= d;
            onceki = p[i];
            i += 1;
        }
    }
    while out.len() < n {
        out.push(p[p.len() - 1]);
    }
    out
}

/// Köşedeki dönüş açısı (derece, 0 = düz devam).
fn donus(a: P, b: P, c: P) -> f64 {
    let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
    let (lu, lv) = ((u[0] * u[0] + u[1] * u[1]).sqrt(), (v[0] * v[0] + v[1] * v[1]).sqrt());
    if lu < 1e-9 || lv < 1e-9 {
        return 0.0;
    }
    ((u[0] * v[0] + u[1] * v[1]) / (lu * lv)).clamp(-1.0, 1.0).acos().to_degrees()
}

/// En küçük kareler çember uydurma (Kåsa): (merkez, yarıçap, göreli hata).
fn cember_uydur(p: &[P]) -> Option<(P, f64, f64)> {
    let n = p.len() as f64;
    if p.len() < 3 {
        return None;
    }
    let (mut sx, mut sy, mut sxx, mut syy, mut sxy, mut sz, mut sxz, mut syz) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for q in p {
        let z = q[0] * q[0] + q[1] * q[1];
        sx += q[0];
        sy += q[1];
        sxx += q[0] * q[0];
        syy += q[1] * q[1];
        sxy += q[0] * q[1];
        sz += z;
        sxz += q[0] * z;
        syz += q[1] * z;
    }
    // [sxx sxy sx; sxy syy sy; sx sy n] [D E F] = -[sxz syz sz]
    let m = [[sxx, sxy, sx], [sxy, syy, sy], [sx, sy, n]];
    let r = [-sxz, -syz, -sz];
    let det = |m: [[f64; 3]; 3]| m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d0 = det(m);
    if d0.abs() < 1e-12 {
        return None;
    }
    let col = |k: usize| {
        let mut mm = m;
        for i in 0..3 {
            mm[i][k] = r[i];
        }
        det(mm) / d0
    };
    let (dd, ee, ff) = (col(0), col(1), col(2));
    let mx = [-dd / 2.0, -ee / 2.0];
    let r2 = mx[0] * mx[0] + mx[1] * mx[1] - ff;
    if r2 <= 0.0 {
        return None;
    }
    let rr = r2.sqrt();
    let hata = (p.iter().map(|q| (uzak(*q, mx) - rr).powi(2)).sum::<f64>() / n).sqrt() / rr;
    Some((mx, rr, hata))
}

/// Çizimi tanır.
fn tani(ham: &[P], zoom: f64) -> Sekil {
    let l = uzunluk(ham);
    let (a, b) = (ham[0], ham[ham.len() - 1]);
    if l * zoom < 6.0 {
        return Sekil::Cizgi(a, b);
    }
    let ab = uzak(a, b);
    // 1) Düz / eğik çizgi: noktalar uç uca çizgiden az sapıyor.
    let sapma = ham.iter().map(|q| parca_uzakligi(*q, a, b)).fold(0.0, f64::max);
    if ab > 0.6 * l && sapma < (0.07 * ab).max(3.0 / zoom) {
        return Sekil::Cizgi(a, aci_yakala(a, b));
    }
    let p = yeniden_ornekle(ham, 160);
    let kapali = ab < 0.2 * l;
    if kapali {
        // Köşeler (kapalı yol, başlangıç noktası köşe olmayabilir: yol kaydırılarak iki kez bakılır).
        let mut halka = p.clone();
        halka.push(p[0]);
        let v = rdp(&halka, 0.03 * l);
        let mut kose: Vec<P> = v[..v.len() - 1].to_vec();
        if kose.len() >= 3 {
            // Başlangıç noktasındaki "sahte" köşeyi at.
            let n = kose.len();
            if donus(kose[n - 1], kose[0], kose[1]) < 30.0 {
                kose.remove(0);
            }
        }
        let n = kose.len();
        let keskin = (0..n).filter(|&i| donus(kose[(i + n - 1) % n], kose[i], kose[(i + 1) % n]) > 35.0).count();
        let (el, el_hata) = elips_uydur(&p);
        if (3..=8).contains(&n) && keskin == n && (el_hata > 0.06 || n <= 4) {
            if n == 4 && (0..4).all(|i| (donus(kose[(i + 3) % 4], kose[i], kose[(i + 1) % 4]) - 90.0).abs() < 25.0) {
                return dikdortgen(&kose, &p);
            }
            return Sekil::Coklu(kose, true);
        }
        return el;
    }
    // 2) Açık: önce yay (çembere oturuyorsa), sonra köşeli çizgi, yoksa yumuşak eğri.
    if let Some((m, r, hata)) = cember_uydur(&p)
        && hata < 0.05
        && r < 4.0 * l
    {
        return Sekil::Egri(yay(&p, m, r));
    }
    // Köşeli: kaba ve ince sadeleştirme hemen hemen aynı köşeleri verir (dönüş köşede toplanmış) ve köşeler
    // keskin. Yumuşak eğride dönüş yayılır: ince sadeleştirme çok daha fazla nokta bırakır.
    let v = rdp(&p, 0.035 * l);
    let ince = rdp(&p, 0.012 * l);
    if v.len() >= 3 && v.len() <= 7 && ince.len() <= v.len() + 2 && (1..v.len() - 1).all(|i| donus(v[i - 1], v[i], v[i + 1]) > 40.0) {
        let mut v = v;
        let son = v.len() - 1;
        v[0] = a;
        v[son] = b;
        return Sekil::Coklu(v, false);
    }
    Sekil::Egri(catmull_rom(&ince))
}

/// Yataya / dikeye çok yakın çizgi tam düz olur.
fn aci_yakala(a: P, b: P) -> P {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let aci = dy.atan2(dx).to_degrees();
    let yakin = |hedef: f64| ((aci - hedef + 540.0) % 360.0 - 180.0).abs() < 3.0;
    if yakin(0.0) || yakin(180.0) {
        [b[0], a[1]]
    } else if yakin(90.0) || yakin(-90.0) {
        [a[0], b[1]]
    } else {
        b
    }
}

/// Elips (PCA): şekil ve göreli hata. Yarı eksenler neredeyse eşitse tam daire.
fn elips_uydur(p: &[P]) -> (Sekil, f64) {
    let n = p.len() as f64;
    let m = [p.iter().map(|q| q[0]).sum::<f64>() / n, p.iter().map(|q| q[1]).sum::<f64>() / n];
    let (mut cxx, mut cyy, mut cxy) = (0.0, 0.0, 0.0);
    for q in p {
        let (x, y) = (q[0] - m[0], q[1] - m[1]);
        cxx += x * x;
        cyy += y * y;
        cxy += x * y;
    }
    cxx /= n;
    cyy /= n;
    cxy /= n;
    let aci = 0.5 * (2.0 * cxy).atan2(cxx - cyy);
    let (c, s) = (aci.cos(), aci.sin());
    // Eksenler boyunca uzanım: noktaların izdüşümlerinin ortalama mutlak yarıçapı (el çizimine dayanıklı).
    let (mut ru, mut rv) = (0.0f64, 0.0f64);
    let mut kutup = Vec::with_capacity(p.len());
    for q in p {
        let (x, y) = (q[0] - m[0], q[1] - m[1]);
        let (u, v) = (x * c + y * s, -x * s + y * c);
        ru = ru.max(u.abs());
        rv = rv.max(v.abs());
        kutup.push((u, v));
    }
    // Uç noktalar yerine ortalamaya yakın: 0,95 ile kırp.
    let (mut rx, mut ry) = (ru * 0.97, rv * 0.97);
    let hata = |rx: f64, ry: f64| (kutup.iter().map(|(u, v)| (((u / rx).powi(2) + (v / ry).powi(2)).sqrt() - 1.0).powi(2)).sum::<f64>() / n).sqrt();
    let h = hata(rx.max(1e-6), ry.max(1e-6));
    if (rx - ry).abs() / rx.max(ry).max(1e-6) < 0.12 {
        let r = (rx + ry) / 2.0;
        rx = r;
        ry = r;
        return (Sekil::Elips { m, rx, ry, aci: 0.0 }, h);
    }
    // Eksen yataya / dikeye çok yakınsa tam hizalı.
    let mut aci = aci;
    let derece = aci.to_degrees();
    for hedef in [-180.0, -90.0, 0.0, 90.0, 180.0] {
        if (derece - hedef).abs() < 4.0 {
            aci = f64::to_radians(hedef);
        }
    }
    (Sekil::Elips { m, rx, ry, aci }, h)
}

/// Dört köşeden düzgün dikdörtgen (en uzun kenarın yönünde; yataya / dikeye yakınsa hizalı).
fn dikdortgen(k: &[P], p: &[P]) -> Sekil {
    let mut en = 0.0;
    let mut aci = 0.0f64;
    for i in 0..4 {
        let (a, b) = (k[i], k[(i + 1) % 4]);
        let l = uzak(a, b);
        if l > en {
            en = l;
            aci = (b[1] - a[1]).atan2(b[0] - a[0]);
        }
    }
    // 90°'nin katına göre en yakın yatay yön.
    let mut a90 = aci.rem_euclid(std::f64::consts::FRAC_PI_2);
    if a90 > std::f64::consts::FRAC_PI_4 {
        a90 -= std::f64::consts::FRAC_PI_2;
    }
    if a90.to_degrees().abs() < 5.0 {
        a90 = 0.0;
    }
    let (c, s) = (a90.cos(), a90.sin());
    let m = [k.iter().map(|q| q[0]).sum::<f64>() / 4.0, k.iter().map(|q| q[1]).sum::<f64>() / 4.0];
    // Yolun döndürülmüş çerçevedeki uzanımı (köşeler yerine bütün yol: el çizimine dayanıklı, %90 kırpılmış).
    let mut us: Vec<f64> = p.iter().map(|q| (q[0] - m[0]) * c + (q[1] - m[1]) * s).collect();
    let mut vs: Vec<f64> = p.iter().map(|q| -(q[0] - m[0]) * s + (q[1] - m[1]) * c).collect();
    us.sort_by(f64::total_cmp);
    vs.sort_by(f64::total_cmp);
    let yuzde = |v: &[f64], t: f64| v[((v.len() - 1) as f64 * t).round() as usize];
    let (u0, u1, v0, v1) = (yuzde(&us, 0.03), yuzde(&us, 0.97), yuzde(&vs, 0.03), yuzde(&vs, 0.97));
    let geri = |u: f64, v: f64| [m[0] + u * c - v * s, m[1] + u * s + v * c];
    Sekil::Coklu(vec![geri(u0, v0), geri(u1, v0), geri(u1, v1), geri(u0, v1)], true)
}

/// Yay: çember üzerinde, çizimin yönünde, başlangıçtan bitişe.
fn yay(p: &[P], m: P, r: f64) -> Vec<P> {
    let aci = |q: &P| (q[1] - m[1]).atan2(q[0] - m[0]);
    let mut toplam = 0.0;
    for w in p.windows(2) {
        let mut d = aci(&w[1]) - aci(&w[0]);
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        toplam += d;
    }
    let a0 = aci(&p[0]);
    let n = ((toplam.abs() * r / 2.0) as usize).clamp(24, 720);
    (0..=n).map(|i| a0 + toplam * i as f64 / n as f64).map(|t| [m[0] + r * t.cos(), m[1] + r * t.sin()]).collect()
}

/// Noktalardan geçen yumuşak eğri.
fn catmull_rom(v: &[P]) -> Vec<P> {
    if v.len() < 3 {
        return v.to_vec();
    }
    let mut out = Vec::new();
    let n = v.len();
    for i in 0..n - 1 {
        let p0 = v[i.saturating_sub(1)];
        let (p1, p2) = (v[i], v[i + 1]);
        let p3 = v[(i + 2).min(n - 1)];
        let adim = ((uzak(p1, p2) / 2.0) as usize).clamp(4, 64);
        for k in 0..adim {
            let t = k as f64 / adim as f64;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f64, b: f64, c: f64, d: f64| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    out.push(v[n - 1]);
    out
}

/// Serbest çizim (bekletilmeden bırakıldı): hafif yumuşatma, uçlar yerinde.
fn yumusat(p: &[P]) -> Vec<P> {
    if p.len() < 4 {
        return p.to_vec();
    }
    let mut out = p.to_vec();
    for _ in 0..2 {
        let onceki = out.clone();
        for i in 1..onceki.len() - 1 {
            out[i] = [(onceki[i - 1][0] + 2.0 * onceki[i][0] + onceki[i + 1][0]) / 4.0, (onceki[i - 1][1] + 2.0 * onceki[i][1] + onceki[i + 1][1]) / 4.0];
        }
    }
    out
}

/// Şeklin çizilecek yolu ve kapalı olup olmadığı.
fn sekil_yolu(s: &Sekil) -> (Vec<P>, bool) {
    match s {
        Sekil::Cizgi(a, b) => (vec![*a, *b], false),
        Sekil::Coklu(v, k) => (v.clone(), *k),
        Sekil::Egri(v) => (v.clone(), false),
        Sekil::Elips { m, rx, ry, aci } => {
            let cevre = std::f64::consts::PI * (3.0 * (rx + ry) - ((3.0 * rx + ry) * (rx + 3.0 * ry)).sqrt());
            let n = ((cevre / 2.0) as usize).clamp(48, 1440);
            let (c, s) = (aci.cos(), aci.sin());
            let pts = (0..n)
                .map(|i| {
                    let t = std::f64::consts::TAU * i as f64 / n as f64;
                    let (u, v) = (rx * t.cos(), ry * t.sin());
                    [m[0] + u * c - v * s, m[1] + u * s + v * c]
                })
                .collect();
            (pts, true)
        }
    }
}

/// Ok: yolun sonu kısaltılır (çizgi ok ucunun içinde biter) ve ok ucu üçgeni. Ok yoksa yol aynen.
fn ok_parcala(yol: &[P], w: f64, ok: bool) -> (Vec<P>, Option<[P; 3]>) {
    if !ok || yol.len() < 2 {
        return (yol.to_vec(), None);
    }
    let boy = (w * 4.0).max(12.0);
    let yari = (w * 2.2).max(6.0);
    let son = yol[yol.len() - 1];
    // Uçtan `boy` kadar geride yol üzerindeki nokta: ok yönü.
    let mut geri = son;
    let mut kalan = boy;
    for i in (0..yol.len() - 1).rev() {
        let d = uzak(yol[i], yol[i + 1]);
        if d >= kalan {
            let t = kalan / d;
            geri = [yol[i + 1][0] + (yol[i][0] - yol[i + 1][0]) * t, yol[i + 1][1] + (yol[i][1] - yol[i + 1][1]) * t];
            kalan = 0.0;
            break;
        }
        kalan -= d;
        geri = yol[i];
    }
    let (dx, dy) = (son[0] - geri[0], son[1] - geri[1]);
    let dl = (dx * dx + dy * dy).sqrt();
    if dl < 1e-6 || kalan > boy * 0.9 {
        return (yol.to_vec(), None);
    }
    let (ux, uy) = (dx / dl, dy / dl);
    let taban = [son[0] - ux * boy, son[1] - uy * boy];
    let bas = [son, [taban[0] - uy * yari, taban[1] + ux * yari], [taban[0] + uy * yari, taban[1] - ux * yari]];
    // Gövde, ok ucunun ortasında biter.
    let kes = boy * 0.5;
    let toplam = uzunluk(yol);
    let mut govde = Vec::new();
    let mut gidilen = 0.0;
    govde.push(yol[0]);
    for w2 in yol.windows(2) {
        let d = uzak(w2[0], w2[1]);
        if gidilen + d >= toplam - kes {
            let t = ((toplam - kes - gidilen) / d.max(1e-9)).clamp(0.0, 1.0);
            govde.push([w2[0][0] + (w2[1][0] - w2[0][0]) * t, w2[0][1] + (w2[1][1] - w2[0][1]) * t]);
            break;
        }
        govde.push(w2[1]);
        gidilen += d;
    }
    (govde, Some(bas))
}

// ------------------------------------------------------------------ pürüzsüz kenarlı çizim (kapsama haritası)

/// Yolu (kalınlık `w`, yuvarlak uçlu) ve varsa ok ucunu belge sınırları içinde kapsama haritasına çizer.
fn rasterle(yol: &[P], kapali: bool, w: f64, ok: bool, dw: i32, dh: i32) -> Option<(photocraft_geom::Rect, Vec<f32>)> {
    let (govde, bas) = ok_parcala(yol, w, ok);
    let r = w / 2.0;
    let mut parcalar: Vec<(P, P)> = govde.windows(2).map(|w| (w[0], w[1])).collect();
    if kapali && govde.len() > 2 {
        parcalar.push((govde[govde.len() - 1], govde[0]));
    }
    if govde.len() == 1 {
        parcalar.push((govde[0], govde[0]));
    }
    let mut x0 = f64::MAX;
    let mut y0 = f64::MAX;
    let mut x1 = f64::MIN;
    let mut y1 = f64::MIN;
    for p in govde.iter().chain(bas.iter().flatten()) {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let bx0 = ((x0 - r - 2.0).floor() as i32).max(0);
    let by0 = ((y0 - r - 2.0).floor() as i32).max(0);
    let bx1 = ((x1 + r + 2.0).ceil() as i32).min(dw);
    let by1 = ((y1 + r + 2.0).ceil() as i32).min(dh);
    if bx1 <= bx0 || by1 <= by0 {
        return None;
    }
    let (bw, bh) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
    let mut k = vec![0f32; bw * bh];
    // Her satır paralel: o satıra değen parçalar için piksel merkezinin parçaya uzaklığı.
    k.par_chunks_mut(bw).enumerate().for_each(|(j, satir)| {
        let py = by0 as f64 + j as f64 + 0.5;
        for (a, b) in &parcalar {
            if py < a[1].min(b[1]) - r - 1.0 || py > a[1].max(b[1]) + r + 1.0 {
                continue;
            }
            let sx0 = ((a[0].min(b[0]) - r - 1.0).floor() as i32 - bx0).max(0) as usize;
            let sx1 = (((a[0].max(b[0]) + r + 1.0).ceil() as i32 - bx0).max(0) as usize).min(bw);
            for (i, h) in satir.iter_mut().enumerate().take(sx1).skip(sx0) {
                let px = bx0 as f64 + i as f64 + 0.5;
                let d = parca_uzakligi([px, py], *a, *b);
                let c = (r + 0.5 - d).clamp(0.0, 1.0) as f32;
                if c > *h {
                    *h = c;
                }
            }
        }
        if let Some(u) = &bas {
            // Dışbükey üçgen: kenarlara işaretli uzaklıkların en küçüğü.
            let alan = (u[1][0] - u[0][0]) * (u[2][1] - u[0][1]) - (u[1][1] - u[0][1]) * (u[2][0] - u[0][0]);
            let yon = if alan >= 0.0 { 1.0 } else { -1.0 };
            for (i, h) in satir.iter_mut().enumerate() {
                let px = bx0 as f64 + i as f64 + 0.5;
                let mut en = f64::MAX;
                for e in 0..3 {
                    let (a, b) = (u[e], u[(e + 1) % 3]);
                    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
                    let l = (ex * ex + ey * ey).sqrt().max(1e-9);
                    let s = yon * (ex * (py - a[1]) - ey * (px - a[0])) / l;
                    en = en.min(s);
                }
                let c = (en + 0.5).clamp(0.0, 1.0) as f32;
                if c > *h {
                    *h = c;
                }
            }
        }
    });
    Some((photocraft_geom::Rect::from_xywh(bx0, by0, bw as u32, bh as u32), k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titrek(f: impl Fn(f64) -> P, n: usize, gurultu: f64) -> Vec<P> {
        (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                let p = f(t);
                let g = ((i * 7919 % 13) as f64 / 13.0 - 0.5) * gurultu;
                [p[0] + g, p[1] - g]
            })
            .collect()
    }

    #[test]
    fn titrek_duz_cizgi_duzlesir() {
        let p = titrek(|t| [100.0 + 300.0 * t, 200.0 + 2.0 * t], 80, 4.0);
        assert!(matches!(tani(&p, 1.0), Sekil::Cizgi(a, b) if (a[1] - b[1]).abs() < 1e-9));
    }

    #[test]
    fn titrek_daire_daire_olur() {
        let p = titrek(|t| [300.0 + 100.0 * (t * 6.2).cos(), 300.0 + 100.0 * (t * 6.2).sin()], 120, 5.0);
        assert!(matches!(tani(&p, 1.0), Sekil::Elips { rx, ry, .. } if (rx - ry).abs() < 1e-9 && (rx - 100.0).abs() < 12.0));
    }

    #[test]
    fn titrek_kare_dikdortgen_olur() {
        let kose = [[100.0, 100.0], [300.0, 100.0], [300.0, 300.0], [100.0, 300.0], [100.0, 105.0]];
        let mut p = Vec::new();
        for w in kose.windows(2) {
            for i in 0..40 {
                let t = i as f64 / 40.0;
                p.push([w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t]);
            }
        }
        let p = titrek(|t| p[((p.len() - 1) as f64 * t) as usize], 160, 3.0);
        match tani(&p, 1.0) {
            Sekil::Coklu(v, true) => {
                assert_eq!(v.len(), 4);
                assert!((v[0][1] - v[1][1]).abs() < 1e-6, "kenar yatay olmalı: {v:?}");
            }
            _ => panic!("dikdörtgen bekleniyordu"),
        }
    }
}
