//! EVREN eklentisi: 360 derece katmanının arayüzü.
//! - Katmanlar paneli sağ tık: "360 Derece Yap" / "360 Ayarları…" / "360'ı Kapat".
//! - Taşıma aracı + etkin katman 360: tuvalde sürüklemek görünümü çevirir (görüntü imleçle kayar).
//! - "360 Ayarları" penceresi: Görüş açısı (FOV), Yatay, Dikey, Eğim, Sıfırla, 360'ı Kapat.
//!
//! Komutlar motorda: `photocraft_engine::evren_pano_cmds` (evren.pano.*).

use std::sync::Mutex;
use std::sync::PoisonError;

use photocraft_doc::{Layer, LayerContent, LayerId};
use photocraft_engine::evren_pano_cmds::{self as pano, View};
use serde_json::{Value, json};

use crate::canvas::ToolEvent;
use crate::t;
use crate::{PhotocraftApp, Tool};

fn window_id() -> egui::Id {
    egui::Id::unique("evren-pano-window")
}

/// Ayarlar penceresini aç.
pub fn open_window(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(window_id(), true));
}

pub fn window_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(window_id())).unwrap_or(false)
}

pub fn close_window(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(window_id(), false));
}

/// Geri alma adımlarını birleştirme anahtarı: bir sürükleme (ya da bir kaydırıcı hareketi) = tek adım.
static STEP: Mutex<u64> = Mutex::new(0);

fn next_key(kind: &str) -> String {
    let mut n = STEP.lock().unwrap_or_else(PoisonError::into_inner);
    *n = n.wrapping_add(1);
    format!("evren-pano-{kind}-{}", *n)
}

fn active_pano(app: &PhotocraftApp) -> Option<(LayerId, View, u32)> {
    let st = app.session.active()?;
    let id = st.active_layer?;
    let l = st.doc.layer(id)?;
    Some((id, pano::view_of(l)?, st.doc.size.width.max(1)))
}

fn report(app: &mut PhotocraftApp, r: Result<Value, String>) {
    if let Err(e) = r {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}

/// Katmanlar paneli sağ tık menüsüne 360 satırları (layer_menu_ui::show içinden çağrılır).
pub fn layer_menu(app: &PhotocraftApp, ui: &mut egui::Ui, l: &Layer, on_set: bool, actions: &mut Vec<(String, Value)>) {
    if on_set {
        return;
    }
    let is_active = app.session.active().is_some_and(|s| s.active_layer == Some(l.id));
    let pano_layer = pano::is_pano(l);
    let raster = matches!(l.content, LayerContent::Raster(_));
    if !pano_layer && !raster {
        return;
    }
    ui.separator();
    let select = |actions: &mut Vec<(String, Value)>| {
        if !is_active {
            actions.push(("layer.select".into(), json!({"layer": l.id.0})));
        }
    };
    if pano_layer {
        if ui.button(t!("360 Settings…", "360 Ayarları…")).clicked() {
            select(actions);
            open_window(ui.ctx());
            ui.close();
        }
        if ui.button(t!("Turn Off 360", "360'ı Kapat")).clicked() {
            select(actions);
            actions.push((pano::OFF.into(), Value::Null));
            ui.close();
        }
    } else if ui.button(t!("Make 360", "360 Derece Yap")).clicked() {
        select(actions);
        actions.push((pano::MAKE.into(), Value::Null));
        open_window(ui.ctx());
        ui.close();
    }
}

struct Drag {
    layer: LayerId,
    start: [f64; 2],
    view: View,
    width: f64,
    key: String,
    last: Option<(f64, f64)>,
}

static DRAG: Mutex<Option<Drag>> = Mutex::new(None);

/// Taşıma aracıyla 360 katmanını çevirme (tool_input::tool_event içinden). `true` = olay kullanıldı.
pub fn pointer(app: &mut PhotocraftApp, tool: Tool, ev: ToolEvent, mods: egui::Modifiers) -> bool {
    let mut drag = DRAG.lock().unwrap_or_else(PoisonError::into_inner);
    match ev {
        ToolEvent::Down { x, y, .. } => {
            *drag = None;
            if tool != Tool::Move || app.ui.transform.is_some() || mods.command || mods.alt {
                return false;
            }
            let Some((layer, view, width)) = active_pano(app) else { return false };
            *drag = Some(Drag { layer, start: [x, y], view, width: f64::from(width), key: next_key("drag"), last: None });
            true
        }
        ToolEvent::Move { x, y, .. } => {
            let Some(d) = drag.as_mut() else { return false };
            // Görüntü imleçle birlikte kayar: belgede 1 px = FOV / belge genişliği derece.
            let k = d.view.fov / d.width;
            let yaw = d.view.yaw - (x - d.start[0]) * k;
            let pitch = d.view.pitch + (y - d.start[1]) * k;
            if d.last == Some((yaw, pitch)) {
                return true;
            }
            d.last = Some((yaw, pitch));
            let p = json!({"layer": d.layer.0, "yaw": yaw, "pitch": pitch, "fast": true, "coalesce": d.key});
            drop(drag);
            let r = app.run(pano::VIEW, p);
            report(app, r);
            true
        }
        ToolEvent::Up { .. } => {
            let Some(d) = drag.take() else { return false };
            drop(drag);
            // Bırakınca tam kalitede yeniden çizilir (aynı geri alma adımında).
            if let Some((yaw, pitch)) = d.last {
                let r = app.run(pano::VIEW, json!({"layer": d.layer.0, "yaw": yaw, "pitch": pitch, "fast": false, "coalesce": d.key}));
                report(app, r);
            }
            true
        }
    }
}

/// Bir kaydırıcı satırı: kaydırıcı + sayı kutusu. Değiştiyse yeni değer ve "hâlâ sürükleniyor mu".
fn row(ui: &mut egui::Ui, label: &str, value: f64, range: std::ops::RangeInclusive<f64>, suffix: &str) -> Option<(f64, bool)> {
    let mut v = value;
    let mut out = None;
    ui.label(label);
    let s = ui.add(egui::Slider::new(&mut v, range.clone()).show_value(false));
    if s.changed() {
        out = Some((v, s.dragged()));
    }
    if s.drag_stopped() {
        out = Some((v, false));
    }
    let mut n = v;
    let b = ui.add(egui::DragValue::new(&mut n).range(range).speed(0.2).max_decimals(1).suffix(suffix));
    if b.changed() {
        out = Some((n, b.dragged()));
    }
    if b.drag_stopped() {
        out = Some((n, false));
    }
    ui.end_row();
    out
}

/// 35 mm eşdeğeri odak uzaklığı (yatay FOV'dan, 36 mm film genişliği).
fn mm(fov: f64) -> f64 {
    18.0 / (fov.to_radians() / 2.0).tan()
}

/// Pencerenin hedefi: etkin katman 360 ise o, değilse belgedeki ilk 360 katmanı.
fn window_target(app: &PhotocraftApp) -> Option<(LayerId, View)> {
    let st = app.session.active()?;
    if let Some(l) = st.active_layer.and_then(|id| st.doc.layer(id))
        && let Some(v) = pano::view_of(l)
    {
        return Some((l.id, v));
    }
    st.doc.walk().into_iter().find_map(|(_, _, l)| pano::view_of(l).map(|v| (l.id, v)))
}

fn pin_color() -> egui::Color32 {
    let [r, g, b] = pano::PIN_COLOR;
    egui::Color32::from_rgb(r, g, b)
}

/// Turuncu pin çizimi (ucu `tip`, yükseklik `h`).
fn paint_pin(p: &egui::Painter, tip: egui::Pos2, h: f32) {
    let r = h * 0.36;
    let c = egui::pos2(tip.x, tip.y - h + r);
    let d = tip.y - c.y;
    let ty = c.y + r * r / d;
    let half = (r * r - (ty - c.y).powi(2)).max(0.0).sqrt();
    let fill = pin_color();
    p.add(egui::Shape::convex_polygon(vec![egui::pos2(tip.x - half, ty), egui::pos2(tip.x + half, ty), tip], fill, egui::Stroke::NONE));
    p.circle_filled(c, r, fill);
    p.circle_filled(c, r * 0.38, egui::Color32::from_rgb(150, 64, 0));
}

/// "360 Ayarları" penceresi (her karede app/frame.rs'ten çağrılır).
pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !window_open(ctx) {
        return;
    }
    let mut open = true;
    let mut run: Option<Value> = None;
    let mut action: Option<(&'static str, Value)> = None;
    let target = window_target(app);
    let has_doc = app.session.active().is_some();
    let can_make = app.session.active().and_then(|st| st.active_layer.and_then(|id| st.doc.layer(id))).is_some_and(|l| matches!(l.content, LayerContent::Raster(_)));
    let mut pin_drop: Option<egui::Pos2> = None;
    let mut pin_click = false;
    let mut export_click = false;
    let resp = egui::Window::new(t!("360 Settings", "360 Ayarları")).id(window_id().with("w")).open(&mut open).resizable(false).collapsible(false).default_width(300.0).show(ctx, |ui| {
        match target {
            None => {
                ui.label(t!("The document has no 360 layer.", "Belgede 360 katmanı yok."));
                ui.label(t!("Right-click a layer and choose \"Make 360\".", "Katmana sağ tıklayıp \"360 Derece Yap\" seçin."));
                ui.add_space(4.0);
                if ui.add_enabled(can_make, egui::Button::new(t!("Make the active layer 360", "Etkin katmanı 360 yap"))).clicked() {
                    action = Some((pano::MAKE, json!({})));
                }
            }
            Some((layer, view)) => {
                ui.strong(t!("View", "Görünüm"));
                ui.label(t!("You can also turn it by dragging on the canvas with the Move tool.", "Taşıma aracıyla tuvalde sürükleyerek de çevirebilirsiniz."));
                ui.add_space(4.0);
                let mut new = view;
                let mut fast = false;
                let mut changed = false;
                egui::Grid::new("evren-pano-grid").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                    for (label, value, range, slot) in [
                        (t!("Field of view", "Görüş açısı"), view.fov, pano::FOV_MIN..=pano::FOV_MAX, 0),
                        (t!("Pan", "Yatay"), view.yaw, -180.0..=180.0, 1),
                        (t!("Tilt", "Dikey"), view.pitch, -pano::PITCH_MAX..=pano::PITCH_MAX, 2),
                        (t!("Roll", "Eğim"), view.roll, -pano::ROLL_MAX..=pano::ROLL_MAX, 3),
                    ] {
                        if let Some((v, f)) = row(ui, label, value, range, "°") {
                            match slot {
                                0 => new.fov = v,
                                1 => new.yaw = v,
                                2 => new.pitch = v,
                                _ => new.roll = v,
                            }
                            fast |= f;
                            changed = true;
                        }
                    }
                });
                ui.label(format!("{}: {:.0} mm", t!("Focal length (35 mm)", "Odak uzaklığı (35 mm)"), mm(view.fov)));
                ui.add_space(4.0);
                if ui.button(t!("Reset", "Sıfırla")).clicked() {
                    new = View { yaw: 0.0, pitch: 0.0, roll: 0.0, fov: 90.0, ..view };
                    changed = true;
                    fast = false;
                }
                if changed && new != view {
                    // Bir kaydırıcı hareketi tek geri alma adımı: anahtar sürükleme bitince yenilenir.
                    let key_id = egui::Id::unique("evren-pano-slider-key");
                    let key = ui.ctx().data_mut(|d| d.get_temp::<String>(key_id)).unwrap_or_else(|| next_key("slider"));
                    if fast {
                        ui.ctx().data_mut(|d| d.insert_temp(key_id, key.clone()));
                    } else {
                        ui.ctx().data_mut(|d| d.remove::<String>(key_id));
                    }
                    run = Some(json!({"layer": layer.0, "yaw": new.yaw, "pitch": new.pitch, "roll": new.roll, "fov": new.fov, "fast": fast, "coalesce": key}));
                }
            }
        }
        if has_doc {
            ui.separator();
            ui.strong("Pin");
            ui.horizontal(|ui| {
                let id = egui::Id::unique("evren-pano-pin-drag");
                let r = ui.dnd_drag_source(id, (), |ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 40.0), egui::Sense::hover());
                    paint_pin(ui.painter(), egui::pos2(rect.center().x, rect.bottom() - 2.0), 36.0);
                });
                let r = r.response.on_hover_text(t!("Drag it onto the canvas, or click", "Sürükleyip tuvale bırakın ya da tıklayın"));
                if r.drag_stopped() {
                    pin_drop = ui.ctx().pointer_latest_pos();
                } else if r.clicked() {
                    pin_click = true;
                }
                ui.label(t!("Drag onto the canvas\n(a click adds it in the middle)", "Sürükleyip tuvale bırakın\n(tıklayınca ortaya eklenir)"));
            });
        }
        if let Some((layer, _)) = target {
            ui.separator();
            ui.strong(t!("Layer", "Katman"));
            ui.horizontal(|ui| {
                if ui.button("360 Export…").on_hover_text(t!("Saves the whole sphere (like a DJI 360 photo)", "Kürenin tamamını kaydeder (DJI 360 fotoğrafı gibi)")).clicked() {
                    export_click = true;
                }
                if ui.button(t!("Turn Off 360", "360'ı Kapat")).on_hover_text(t!("Puts the flat panorama back", "Düz panoramayı geri koyar")).clicked() {
                    action = Some((pano::OFF, json!({"layer": layer.0})));
                }
            });
        }
    });
    if !open {
        close_window(ctx);
    }
    if let Some(p) = run {
        let r = app.run(pano::VIEW, p);
        report(app, r);
    }
    if let Some((id, p)) = action {
        let r = app.run(id, p);
        report(app, r);
    }
    if export_click && let Some(r) = invoke(app, pano::EXPORT, &json!({})) {
        report(app, r);
    }
    // Pin bırakıldı: pencerenin dışında, tuvalin üzerindeyse ucu o noktaya.
    let over_window = |pos: egui::Pos2| resp.as_ref().is_some_and(|w| w.response.rect.contains(pos));
    if let Some(pos) = pin_drop.filter(|p| !over_window(*p))
        && let Some(xf) = crate::canvas::ViewXform::active(app)
        && xf.rect.contains(pos)
    {
        let [x, y] = xf.to_doc(pos);
        let r = app.run(pano::PIN, json!({"x": x, "y": y}));
        report(app, r);
    } else if pin_click {
        let r = app.run(pano::PIN, json!({}));
        report(app, r);
    }
}

/// Katmanlar panelinde 360 katmanının küçük resmine turuncu "360" rozeti (panels/layer_row.rs'ten).
pub fn row_badge(ui: &egui::Ui, l: &Layer, thumb: egui::Rect) {
    if !pano::is_pano(l) {
        return;
    }
    let h = (thumb.height() * 0.42).clamp(10.0, 14.0);
    let w = h * 1.9;
    let r = egui::Rect::from_min_size(egui::pos2(thumb.right() - w + 2.0, thumb.bottom() - h + 2.0), egui::vec2(w, h));
    let p = ui.painter();
    p.rect_filled(r, 3.0, pin_color());
    p.text(r.center(), egui::Align2::CENTER_CENTER, "360", egui::FontId::proportional(h * 0.78), egui::Color32::WHITE);
}

/// Menüden çağrılan EVREN komutları (menus/invoke.rs'ten): kayıt yeri sorulması gerekenler.
pub fn invoke(app: &mut PhotocraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id != pano::EXPORT || params.get("path").is_some() {
        return None;
    }
    let stem = app
        .session
        .active()
        .map(|st| std::path::Path::new(&st.doc.name).file_stem().and_then(|s| s.to_str()).unwrap_or("360").to_string())
        .unwrap_or_else(|| "360".into());
    Some(app.pick_save(&format!("{stem}_360.jpg"), |app, path| {
        let r = app.run(pano::EXPORT, json!({"path": path}));
        if let Ok(v) = &r {
            app.ui.status = format!("{}: {}", t!("360 Export saved", "360 Export kaydedildi"), v["path"].as_str().unwrap_or_default());
            app.ui.status_error = false;
        }
        r
    }))
}
