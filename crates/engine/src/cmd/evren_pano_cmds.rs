//! EVREN eklentisi: 360 derece (equirectangular, 2:1 küre fotoğrafı) katmanı.
//!
//! "360 Derece Yap" bir piksel katmanının düz panoramasını katmanın içine saklar ve katmanı
//! belge boyutunda perspektif bir görünümle değiştirir. Görünüm yatay (yaw), dikey (pitch),
//! eğim (roll) ve yatay görüş açısıyla (FOV) değişir; Taşıma aracıyla sürüklemek çevirir.
//!
//! Saklama (güncellemelerde bozulmasın diye belge modeline alan eklenmedi): katmanın
//! `psd_blocks` listesinde iki blok. `evPs` = düz panorama (JPEG), `evPv` = görünüm (JSON).
//! İkisi de `.pcraft`, PSD, PSB ve katmanlı TIFF'te aynen saklanır, katman çoğaltılınca kopyalanır.
//! Döndürme: R = yaw · pitch · roll.

use std::sync::{Arc, Mutex, PoisonError};

use photocraft_color::{ColorMode, PixelFormat};
use photocraft_doc::{Layer, LayerContent, LayerId};
use photocraft_geom::Rect;
use photocraft_raster::Surface;
use rayon::prelude::*;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

pub const MAKE: &str = "evren.pano.make";
pub const VIEW: &str = "evren.pano.view";
pub const INFO: &str = "evren.pano.info";
pub const OFF: &str = "evren.pano.off";
pub const PIN: &str = "evren.pano.pin";
pub const EXPORT: &str = "evren.pano.export";

/// Fotoğrafın kendi bilgileri: `[EXIF uzunluğu u32 BE][EXIF][XMP UTF-8]`.
pub const META_KEY: [u8; 4] = *b"evPm";

/// Düz panoramanın bloğu (JPEG baytları).
pub const SOURCE_KEY: [u8; 4] = *b"evPs";
/// Görünüm bloğu (JSON).
pub const VIEW_KEY: [u8; 4] = *b"evPv";

pub const FOV_MIN: f64 = 10.0;
pub const FOV_MAX: f64 = 150.0;
pub const PITCH_MAX: f64 = 90.0;
pub const ROLL_MAX: f64 = 45.0;

/// Kabul edilen en büyük panorama (piksel): 2 × 16384 × 8192.
const MAX_SOURCE_PIXELS: u64 = 2 * 16384 * 8192;
/// Kabul edilen en büyük belge (piksel) görünüm çizimi için.
const MAX_VIEW_PIXELS: u64 = 1 << 28;

/// 360 katmanının görünümü (derece) ve düz panoramanın eski yeri (360'ı kapatınca geri konur).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub fov: f64,
    pub x: i32,
    pub y: i32,
}

impl Default for View {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.0, roll: 0.0, fov: 90.0, x: 0, y: 0 }
    }
}

impl View {
    /// Sınırlar: yaw -180..180 (sarılır), pitch ±90, roll ±45, FOV 10..150.
    pub fn clamped(mut self) -> Self {
        let fin = |v: f64, d: f64| if v.is_finite() { v } else { d };
        self.yaw = (fin(self.yaw, 0.0) + 180.0).rem_euclid(360.0) - 180.0;
        self.pitch = fin(self.pitch, 0.0).clamp(-PITCH_MAX, PITCH_MAX);
        self.roll = fin(self.roll, 0.0).clamp(-ROLL_MAX, ROLL_MAX);
        self.fov = fin(self.fov, 90.0).clamp(FOV_MIN, FOV_MAX);
        self
    }

    fn to_json(self) -> Value {
        json!({"yaw": self.yaw, "pitch": self.pitch, "roll": self.roll, "fov": self.fov, "x": self.x, "y": self.y})
    }

    fn from_json(v: &Value) -> Option<Self> {
        let f = |k: &str, d: f64| v.get(k).and_then(Value::as_f64).unwrap_or(d);
        let i = |k: &str| v.get(k).and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok()).unwrap_or(0);
        v.is_object().then(|| Self { yaw: f("yaw", 0.0), pitch: f("pitch", 0.0), roll: f("roll", 0.0), fov: f("fov", 90.0), x: i("x"), y: i("y") }.clamped())
    }
}

/// Katman 360 ise görünümü.
pub fn view_of(l: &Layer) -> Option<View> {
    let (_, data) = l.psd_blocks.iter().find(|(k, _)| *k == VIEW_KEY)?;
    l.psd_blocks.iter().find(|(k, _)| *k == SOURCE_KEY)?;
    // PSD yazıcısı bloğu 4 baytın katına sıfırla doldurabilir: sondaki sıfırlar atılır.
    let end = data.iter().rposition(|b| *b != 0 && !b.is_ascii_whitespace()).map_or(0, |i| i + 1);
    let v: Value = serde_json::from_slice(data.get(..end)?).ok()?;
    View::from_json(&v)
}

pub fn is_pano(l: &Layer) -> bool {
    view_of(l).is_some()
}

/// Çözülmüş düz panorama (RGBA8).
pub struct Equi {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[u8; 4]>,
}

/// Son çözülen panoramalar (çevirirken her seferinde JPEG çözülmesin). Anahtar bloğun Arc'ı:
/// geçmiş adımları ve çoğaltılan katmanlar aynı Arc'ı paylaşır. Arc burada tutulduğu için adresi
/// başka bir bloğa geçemez.
type CacheEntry = (Arc<Vec<u8>>, Arc<Equi>);
static CACHE: Mutex<Vec<CacheEntry>> = Mutex::new(Vec::new());
const CACHE_LEN: usize = 2;

fn cache_put(bytes: Arc<Vec<u8>>, e: Arc<Equi>) {
    let mut c = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    c.retain(|(b, _)| !Arc::ptr_eq(b, &bytes));
    c.insert(0, (bytes, e));
    c.truncate(CACHE_LEN);
}

fn decode_source(bytes: &Arc<Vec<u8>>) -> Result<Arc<Equi>> {
    {
        let c = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, e)) = c.iter().find(|(b, _)| Arc::ptr_eq(b, bytes)) {
            return Ok(e.clone());
        }
    }
    let img = photocraft_codecs::decode(bytes).map_err(|e| EngineError::Other(format!("{}: {e}", crate::evren_ai_cmds::t("Could not decode the 360 image", "360 görsel çözülemedi"))))?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_SOURCE_PIXELS {
        return Err(EngineError::Other(crate::evren_ai_cmds::t("Invalid 360 image size", "360 görselin boyutu geçersiz").into()));
    }
    let raw = img.to_rgba8();
    let px: Vec<[u8; 4]> = raw.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
    if px.len() != w as usize * h as usize {
        return Err(EngineError::Other(crate::evren_ai_cmds::t("The 360 image data is incomplete", "360 görselin verisi eksik").into()));
    }
    let e = Arc::new(Equi { w, h, px });
    cache_put(bytes.clone(), e.clone());
    Ok(e)
}

/// R = Ry(yaw) · Rx(pitch) · Rz(roll), satır sıralı.
fn rotation(v: &View) -> [f64; 9] {
    let r = std::f64::consts::PI / 180.0;
    let (sy, cy) = (v.yaw * r).sin_cos();
    let (sx, cx) = (v.pitch * r).sin_cos();
    let (sz, cz) = (v.roll * r).sin_cos();
    let ry = [cy, 0.0, sy, 0.0, 1.0, 0.0, -sy, 0.0, cy];
    let rx = [1.0, 0.0, 0.0, 0.0, cx, sx, 0.0, -sx, cx];
    let rz = [cz, -sz, 0.0, sz, cz, 0.0, 0.0, 0.0, 1.0];
    mul(&mul(&ry, &rx), &rz)
}

fn mul(a: &[f64; 9], b: &[f64; 9]) -> [f64; 9] {
    let mut c = [0.0; 9];
    for i in 0..3 {
        for j in 0..3 {
            c[i * 3 + j] = a[i * 3] * b[j] + a[i * 3 + 1] * b[3 + j] + a[i * 3 + 2] * b[6 + j];
        }
    }
    c
}

/// Çift doğrusal örnek; x sarılır (boylam ±180 dikişi), y kenarda kalır. Sonuç 0..255 float.
#[inline]
fn sample(e: &Equi, u: f64, v: f64) -> [f32; 4] {
    let w = e.w as i64;
    let h = e.h as i64;
    let fx = u - 0.5;
    let fy = (v - 0.5).clamp(0.0, (h - 1) as f64);
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = (fx - x0) as f32;
    let ty = (fy - y0) as f32;
    let x0 = (x0 as i64).rem_euclid(w);
    let x1 = (x0 + 1).rem_euclid(w);
    let y0 = (y0 as i64).clamp(0, h - 1);
    let y1 = (y0 + 1).min(h - 1);
    let at = |x: i64, y: i64| e.px.get((y * w + x) as usize).copied().unwrap_or([0, 0, 0, 0]);
    let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    let mut o = [0.0f32; 4];
    for k in 0..4 {
        let top = f32::from(a[k]) + (f32::from(b[k]) - f32::from(a[k])) * tx;
        let bot = f32::from(c[k]) + (f32::from(d[k]) - f32::from(c[k])) * tx;
        o[k] = top + (bot - top) * ty;
    }
    o
}

/// Görünümü `w`×`h` RGBA8 olarak çizer. `fast` = sürüklerken (piksel başına tek örnek); değilse
/// kaynak çıktıdan sıksa piksel başına n×n örnek (kenar kırılması olmasın).
pub fn render(e: &Equi, view: &View, w: u32, h: u32, fast: bool) -> Vec<u8> {
    let v = view.clamped();
    let m = rotation(&v);
    let t = (v.fov.to_radians() / 2.0).tan();
    let (wf, hf) = (f64::from(w.max(1)), f64::from(h.max(1)));
    let k = [t, t * hf / wf];
    let (sw, sh) = (f64::from(e.w), f64::from(e.h));
    // Bir çıktı pikselinin kaynakta kaç piksel tuttuğu (yatayda).
    let density = sw * v.fov / (360.0 * wf);
    let n: usize = if fast || density <= 1.25 { 1 } else { (density.ceil() as usize).clamp(2, 3) };
    let inv = 1.0 / (n * n) as f32;
    let two_pi = 2.0 * std::f64::consts::PI;
    let pi = std::f64::consts::PI;
    let mut out = vec![0u8; w as usize * h as usize * 4];
    out.par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w as usize {
            let mut acc = [0.0f32; 4];
            for sy in 0..n {
                for sx in 0..n {
                    let px = (x as f64 + (sx as f64 + 0.5) / n as f64) / wf * 2.0 - 1.0;
                    let py = 1.0 - (y as f64 + (sy as f64 + 0.5) / n as f64) / hf * 2.0;
                    let (a, b, c) = (px * k[0], py * k[1], 1.0);
                    let dx = m[0] * a + m[1] * b + m[2] * c;
                    let dy = m[3] * a + m[4] * b + m[5] * c;
                    let dz = m[6] * a + m[7] * b + m[8] * c;
                    let len = (dx * dx + dy * dy + dz * dz).sqrt().max(1e-12);
                    let u = (0.5 + dx.atan2(dz) / two_pi) * sw;
                    let vv = (0.5 - (dy / len).clamp(-1.0, 1.0).asin() / pi) * sh;
                    let s = sample(e, u, vv);
                    for i in 0..4 {
                        acc[i] += s[i];
                    }
                }
            }
            let o = &mut row[x * 4..x * 4 + 4];
            for i in 0..4 {
                o[i] = (acc[i] * inv).round().clamp(0.0, 255.0) as u8;
            }
        }
    });
    out
}

fn bad(cmd: &str, message: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: message.into() }
}

/// `{"layer":id?}`: verilen katman, yoksa etkin katman.
fn target(s: &Session, cmd: &str, p: &Value) -> Result<LayerId> {
    let st = s.active().ok_or(EngineError::NoDocument)?;
    match p.get("layer") {
        Some(Value::Null) | None => st.active_layer.ok_or_else(|| bad(cmd, crate::evren_ai_cmds::t("no active layer", "etkin katman yok"))),
        Some(v) => {
            let id = LayerId(v.as_u64().ok_or_else(|| bad(cmd, "`layer` must be a layer id"))?);
            st.doc.layer(id).ok_or(EngineError::NoLayer(id))?;
            Ok(id)
        }
    }
}

fn rgb_doc(s: &Session) -> std::result::Result<(), String> {
    let d = s.active().ok_or(crate::evren_ai_cmds::t("no document open", "belge açık değil"))?;
    if d.doc.pixel_format().mode != ColorMode::Rgb {
        return Err(crate::evren_ai_cmds::t("360 works only in RGB documents", "360 yalnız RGB belgelerde çalışır").into());
    }
    Ok(())
}

fn make_enabled(s: &Session) -> std::result::Result<(), String> {
    rgb_doc(s)?;
    let l = crate::active_layer_of(s)?;
    if !matches!(l.content, LayerContent::Raster(_)) {
        return Err(crate::evren_ai_cmds::t("360 needs a pixel layer (rasterize the layer first)", "360 yalnız piksel katmanında yapılır (önce katmanı rasterleştirin)").into());
    }
    if is_pano(l) {
        return Err(crate::evren_ai_cmds::t("the layer is already 360", "katman zaten 360").into());
    }
    Ok(())
}

fn pano_enabled(s: &Session) -> std::result::Result<(), String> {
    let l = crate::active_layer_of(s)?;
    if !is_pano(l) {
        return Err(crate::evren_ai_cmds::t("the active layer is not a 360 layer", "etkin katman 360 değil").into());
    }
    Ok(())
}

fn info_enabled(s: &Session) -> std::result::Result<(), String> {
    crate::active_layer_of(s).map(|_| ())
}

/// Görünümü belge boyutunda katman yüzeyine çevirir.
fn view_surface(e: &Equi, v: &View, size: photocraft_geom::Size, format: PixelFormat, fast: bool) -> Result<Surface> {
    let (w, h) = (size.width, size.height);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_VIEW_PIXELS {
        return Err(EngineError::Other(crate::evren_ai_cmds::t("the document is too large for a 360 view", "belge boyutu 360 görünüm için çok büyük").into()));
    }
    let bytes = render(e, v, w, h, fast);
    let rgba = Surface::from_interleaved(PixelFormat::RGBA8, Rect::from_xywh(0, 0, w, h), &bytes);
    if format == PixelFormat::RGBA8 {
        Ok(rgba)
    } else {
        rgba.try_convert(format).map_err(|e| EngineError::Other(format!("{}: {e}", crate::evren_ai_cmds::t("out of memory", "bellek yetmedi"))))
    }
}

fn set_block(l: &mut Layer, key: [u8; 4], data: Arc<Vec<u8>>) {
    if let Some(b) = l.psd_blocks.iter_mut().find(|(k, _)| *k == key) {
        b.1 = data;
    } else {
        l.psd_blocks.push((key, data));
    }
}

fn view_block(v: &View) -> Arc<Vec<u8>> {
    Arc::new(serde_json::to_vec(&v.to_json()).unwrap_or_default())
}

fn params_view(p: &Value, base: View) -> View {
    let f = |k: &str, d: f64| p.get(k).and_then(Value::as_f64).unwrap_or(d);
    View { yaw: f("yaw", base.yaw), pitch: f("pitch", base.pitch), roll: f("roll", base.roll), fov: f("fov", base.fov), ..base }.clamped()
}

/// Katman › 360 Derece Yap.
fn make(s: &mut Session, p: &Value) -> Result<Value> {
    rgb_doc(s).map_err(|m| bad(MAKE, m))?;
    let id = target(s, MAKE, p)?;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let size = st.doc.size;
    let l = st.doc.layer(id).ok_or(EngineError::NoLayer(id))?;
    if is_pano(l) {
        return Err(bad(MAKE, crate::evren_ai_cmds::t("the layer is already 360", "katman zaten 360")));
    }
    let LayerContent::Raster(surface) = &l.content else {
        return Err(bad(MAKE, crate::evren_ai_cmds::t("360 needs a pixel layer (rasterize the layer first)", "360 yalnız piksel katmanında yapılır (önce katmanı rasterleştirin)")));
    };
    let r = surface.content_bounds();
    if r.is_empty() {
        return Err(bad(MAKE, crate::evren_ai_cmds::t("the layer is empty", "katman boş")));
    }
    let (w, h) = (r.width(), r.height());
    if u64::from(w) * u64::from(h) > MAX_SOURCE_PIXELS {
        return Err(bad(MAKE, crate::evren_ai_cmds::t("the image is too large for 360 (at most 32768×16384)", "görsel 360 için çok büyük (en çok 32768×16384)")));
    }
    let format = surface.format();
    let mut rgba = vec![[0u8; 4]; w as usize * h as usize];
    surface.read_rgba8_into(r, &mut rgba);
    let rgb: Vec<u8> = rgba.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let jpeg = photocraft_codecs::web::encode_jpeg_rgb8(w, h, &rgb, 95, false, true, None, None, None, None)
        .map_err(|e| EngineError::Other(format!("{}: {e}", crate::evren_ai_cmds::t("could not store the 360 image", "360 görsel saklanamadı"))))?;
    drop(rgb);
    let source = Arc::new(jpeg);
    let equi = Arc::new(Equi { w, h, px: rgba });
    cache_put(source.clone(), equi.clone());
    let view = params_view(p, View { x: r.x0, y: r.y0, ..View::default() });
    let new_surface = view_surface(&equi, &view, size, format, false)?;
    let vb = view_block(&view);
    // Fotoğrafın kendi bilgileri (DJI: kamera, GPS, GPano 360 işareti): belge bu panoramanın
    // kendisiyse ya da XMP'si 360 diyorsa 360 Export için katmanda saklanır.
    let meta = &st.doc.metadata;
    let own = (size.width == w && size.height == h) || meta.xmp.as_deref().is_some_and(|x| x.contains("GPano"));
    let meta_block = own.then(|| meta_block(meta.exif.as_deref().map(Vec::as_slice), meta.xmp.as_deref())).flatten();
    s.edit(crate::evren_ai_cmds::t("Make 360", "360 Derece Yap"), move |doc, _| {
        let l = doc.layer_mut(id).ok_or(EngineError::NoLayer(id))?;
        l.content = LayerContent::Raster(new_surface);
        set_block(l, SOURCE_KEY, source);
        set_block(l, VIEW_KEY, vb);
        if let Some(m) = meta_block {
            set_block(l, META_KEY, m);
        }
        Ok(())
    })?;
    Ok(json!({"layer": id.0, "view": view.to_json(), "width": w, "height": h, "ratio": f64::from(w) / f64::from(h)}))
}

/// 360 görünümünü değiştirir: `{"layer":id?,"yaw":deg?,"pitch":deg?,"roll":deg?,"fov":deg?,"fast":bool?}`.
fn view_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, VIEW, p)?;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let size = st.doc.size;
    let l = st.doc.layer(id).ok_or(EngineError::NoLayer(id))?;
    let old = view_of(l).ok_or_else(|| bad(VIEW, crate::evren_ai_cmds::t("the layer is not a 360 layer", "katman 360 değil")))?;
    let LayerContent::Raster(surface) = &l.content else {
        return Err(bad(VIEW, crate::evren_ai_cmds::t("the 360 layer is not a pixel layer", "360 katmanı piksel katmanı değil")));
    };
    let format = surface.format();
    let source = source_of(l).ok_or_else(|| bad(VIEW, crate::evren_ai_cmds::t("360 image not found", "360 görsel bulunamadı")))?;
    let fast = p.get("fast").and_then(Value::as_bool).unwrap_or(false);
    let view = params_view(p, old);
    let mut equi = decode_source(&source)?;
    // Çevirmeden önce katmana doğrudan çizilenler (fırça, yapıştırma, …) küreye işlenir, böylece
    // küreyle birlikte döner. Bir sürüklemede yalnız ilk adımda denetlenir.
    let mut new_source = None;
    if check_due(p) {
        let current = read_view(surface, size);
        if let Some(baked) = bake(&equi, &old, &current, size)? {
            let bytes = Arc::new(encode_equi(&baked)?);
            let baked = Arc::new(baked);
            cache_put(bytes.clone(), baked.clone());
            equi = baked;
            new_source = Some(bytes);
        }
    }
    let new_surface = view_surface(&equi, &view, size, format, fast)?;
    let vb = view_block(&view);
    s.edit(crate::evren_ai_cmds::t("360 View", "360 Görünüm"), move |doc, _| {
        let l = doc.layer_mut(id).ok_or(EngineError::NoLayer(id))?;
        l.content = LayerContent::Raster(new_surface);
        set_block(l, VIEW_KEY, vb);
        if let Some(b) = new_source {
            set_block(l, SOURCE_KEY, b);
        }
        Ok(())
    })?;
    Ok(json!({"layer": id.0, "view": view.to_json()}))
}

/// Son denetlenen birleştirme anahtarı (bir sürükleme boyunca küreye işleme denetimi bir kez).
static LAST_CHECK: Mutex<Option<String>> = Mutex::new(None);

fn check_due(p: &Value) -> bool {
    let key = p.get("coalesce").and_then(Value::as_str).map(str::to_string);
    let mut last = LAST_CHECK.lock().unwrap_or_else(PoisonError::into_inner);
    match key {
        Some(k) if last.as_deref() == Some(k.as_str()) => false,
        k => {
            *last = k;
            true
        }
    }
}

pub fn source_of(l: &Layer) -> Option<Arc<Vec<u8>>> {
    l.psd_blocks.iter().find(|(k, _)| *k == SOURCE_KEY).map(|(_, d)| d.clone())
}

/// Katman yüzeyinin belge alanı (0,0)–(W,H), düz RGBA8.
fn read_view(surface: &Surface, size: photocraft_geom::Size) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; size.width as usize * size.height as usize];
    surface.read_rgba8_into(Rect::from_xywh(0, 0, size.width, size.height), &mut out);
    out
}

fn encode_equi(e: &Equi) -> Result<Vec<u8>> {
    let rgb: Vec<u8> = e.px.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    photocraft_codecs::web::encode_jpeg_rgb8(e.w, e.h, &rgb, 95, false, true, None, None, None, None)
        .map_err(|err| EngineError::Other(format!("{}: {err}", crate::evren_ai_cmds::t("could not store the 360 image", "360 görsel saklanamadı"))))
}

/// Görünüm tamponunda çift doğrusal örnek (kenarda kalır).
fn view_sample<const N: usize>(buf: &[[f32; N]], w: usize, h: usize, x: f64, y: f64) -> [f32; N] {
    let fx = (x - 0.5).clamp(0.0, (w - 1) as f64);
    let fy = (y - 0.5).clamp(0.0, (h - 1) as f64);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
    let g = |xx: usize, yy: usize| buf.get(yy * w + xx).copied().unwrap_or([0.0; N]);
    let (a, b, c, d) = (g(x0, y0), g(x1, y0), g(x0, y1), g(x1, y1));
    let mut o = [0.0f32; N];
    for i in 0..N {
        let top = a[i] + (b[i] - a[i]) * tx;
        let bot = c[i] + (d[i] - c[i]) * tx;
        o[i] = top + (bot - top) * ty;
    }
    o
}

/// Görünümde kürenin o açıdaki çiziminden farklı olan pikselleri küreye işler; fark yoksa `None`.
/// Her küre pikseli için yön → kamera (Rᵀ) → görünümün içindeyse değişmiş görünüm pikseli
/// karıştırılır, değilse küre pikseli aynen kalır.
fn bake(e: &Equi, view: &View, current: &[[u8; 4]], size: photocraft_geom::Size) -> Result<Option<Equi>> {
    let (w, h) = (size.width as usize, size.height as usize);
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_VIEW_PIXELS || current.len() != w * h {
        return Ok(None);
    }
    let reference = render(e, view, size.width, size.height, false);
    // Değişen pikseller: renk 3 düzeyden fazla farklı ya da saydamlık var. [renk + maske]
    let layer: Vec<[f32; 5]> = current
        .par_iter()
        .zip(reference.par_chunks_exact(4))
        .map(|(c, r)| {
            let d = (0..3).map(|i| (i32::from(c[i]) - i32::from(r[i])).abs()).max().unwrap_or(0);
            let m = if d > 3 || c[3] < 250 { 1.0 } else { 0.0 };
            [f32::from(c[0]), f32::from(c[1]), f32::from(c[2]), f32::from(c[3]), m]
        })
        .collect();
    if !layer.iter().any(|p| p[4] > 0.0) {
        return Ok(None);
    }
    let v = view.clamped();
    let m = rotation(&v);
    let t = (v.fov.to_radians() / 2.0).tan();
    let (wf, hf) = (w as f64, h as f64);
    let k = [t, t * hf / wf];
    let (sw, sh) = (e.w as usize, e.h as usize);
    let pi = std::f64::consts::PI;
    let lon: Vec<(f64, f64)> = (0..sw).map(|x| (((x as f64 + 0.5) / sw as f64 - 0.5) * 2.0 * pi).sin_cos()).collect();
    let mut px = e.px.clone();
    px.par_chunks_mut(sw).enumerate().for_each(|(y, row)| {
        let (slat, clat) = ((0.5 - (y as f64 + 0.5) / sh as f64) * pi).sin_cos();
        for (x, out) in row.iter_mut().enumerate() {
            let Some(&(slon, clon)) = lon.get(x) else { continue };
            let d = [clat * slon, slat, clat * clon];
            // Rᵀ · d (R satır sıralı).
            let cx = m[0] * d[0] + m[3] * d[1] + m[6] * d[2];
            let cy = m[1] * d[0] + m[4] * d[1] + m[7] * d[2];
            let cz = m[2] * d[0] + m[5] * d[1] + m[8] * d[2];
            if cz <= 1e-6 {
                continue;
            }
            let (qx, qy) = (cx / (cz * k[0]), cy / (cz * k[1]));
            if qx.abs() > 1.0 || qy.abs() > 1.0 {
                continue;
            }
            let c = view_sample(&layer, w, h, (qx + 1.0) * 0.5 * wf, (1.0 - qy) * 0.5 * hf);
            if c[4] <= 0.0 {
                continue;
            }
            let a = (c[4] * c[3] / 255.0).clamp(0.0, 1.0);
            for i in 0..3 {
                out[i] = (f32::from(out[i]) * (1.0 - a) + c[i] * a).round().clamp(0.0, 255.0) as u8;
            }
        }
    });
    Ok(Some(Equi { w: e.w, h: e.h, px }))
}

/// Filtre / ayarlama komutunu kürenin tamamına uygular: küre ayrı, görünmez bir oturumda tek
/// katmanlı belge olarak açılır ve aynı komut aynı ayarlarla çalıştırılır.
fn replay_on_sphere(cmd: &str, params: &Value, e: &Equi) -> Result<Equi> {
    let mut doc = photocraft_doc::Document::new("360", photocraft_geom::Size::new(e.w, e.h), ColorMode::Rgb, photocraft_color::SampleType::U8);
    let rect = Rect::from_xywh(0, 0, e.w, e.h);
    let bytes: Vec<u8> = e.px.iter().flatten().copied().collect();
    let mut surface = Surface::from_interleaved(PixelFormat::RGBA8, rect, &bytes);
    drop(bytes);
    let format = doc.pixel_format();
    if format != PixelFormat::RGBA8 {
        surface = surface.try_convert(format).map_err(|err| EngineError::Other(format!("{}: {err}", crate::evren_ai_cmds::t("out of memory", "bellek yetmedi"))))?;
    }
    let layer = Layer::new(crate::evren_ai_cmds::t("Sphere", "Küre"), LayerContent::Raster(surface));
    let lid = layer.id;
    doc.layers.push(layer);
    let mut tmp = Session::new();
    tmp.add_document(doc, None);
    if let Some(st) = tmp.active_mut() {
        st.active_layer = Some(lid);
    }
    let mut p = params.clone();
    if let Some(o) = p.as_object_mut() {
        for k in ["layer", "coalesce", "target"] {
            o.remove(k);
        }
    }
    tmp.execute(cmd, p)?;
    let st = tmp.active().ok_or(EngineError::NoDocument)?;
    let l = st.doc.layer(lid).ok_or(EngineError::NoLayer(lid))?;
    let LayerContent::Raster(out) = &l.content else {
        return Err(EngineError::Other("sphere layer changed".into()));
    };
    let mut px = vec![[0u8; 4]; e.px.len()];
    out.read_rgba8_into(rect, &mut px);
    Ok(Equi { w: e.w, h: e.h, px })
}

/// Küreye tamamen uygulanacak komutlar: filtreler ve Görüntü › Ayarlamalar (Camera Raw dahil).
fn whole_sphere_command(id: &str) -> bool {
    (id.starts_with("filter.") || id.starts_with("image.adjustments.")) && id != "filter.lastFilter"
}

/// Her başarılı komuttan sonra (motorun `after_command`'ından, EVREN eklentisi) çağrılır:
/// - 360 katmanına uygulanan filtre / ayarlama kürenin tamamına uygulanır (döndürünce sıfırlanmaz);
/// - 360 katmanına birleştirilen (Merge) katmanlar küreye işlenir, katman 360 kalır.
pub fn after_command(s: &mut Session, id: &str, params: &Value) {
    if id.starts_with("evren.") {
        return;
    }
    let merge = id.starts_with("layer.merge");
    if !merge && !whole_sphere_command(id) {
        return;
    }
    if let Err(e) = after_command_inner(s, id, params, merge) {
        eprintln!("EVREN 360: `{id}` could not be baked into the sphere: {e}");
    }
}

fn after_command_inner(s: &mut Session, id: &str, params: &Value, merge: bool) -> Result<()> {
    let Some(st) = s.active() else { return Ok(()) };
    let Some(before) = st.history.recorded_before() else { return Ok(()) };
    if Arc::ptr_eq(&before, &st.doc) {
        return Ok(());
    }
    let size = st.doc.size;
    let has_selection = st.doc.selection.is_some();
    // Önceki belgedeki 360 katmanlarından bu komutla değişenler.
    let mut todo: Vec<(LayerId, View, Arc<Vec<u8>>, Surface)> = Vec::new();
    for (_, _, old) in before.walk() {
        let (Some(view), Some(src)) = (view_of(old), source_of(old)) else { continue };
        let Some(now) = st.doc.layer(old.id) else { continue };
        let (LayerContent::Raster(now_surface), LayerContent::Raster(old_surface)) = (&now.content, &old.content) else { continue };
        let still_pano = is_pano(now);
        if merge && !still_pano {
            todo.push((old.id, view, src, now_surface.clone()));
        } else if !merge && still_pano && !has_selection && now_surface != old_surface {
            todo.push((old.id, view, src, now_surface.clone()));
        }
    }
    for (lid, view, src, now_surface) in todo {
        let equi = decode_source(&src)?;
        let format = now_surface.format();
        let new_equi = if merge {
            // Birleştirme sonucu: üstteki katmanlar küreye işlenir, 360 bilgisi geri konur.
            match bake(&equi, &view, &read_view(&now_surface, size), size)? {
                Some(e) => Arc::new(e),
                None => equi.clone(),
            }
        } else {
            Arc::new(replay_on_sphere(id, params, &equi)?)
        };
        let bytes = if Arc::ptr_eq(&new_equi, &equi) { src.clone() } else { Arc::new(encode_equi(&new_equi)?) };
        cache_put(bytes.clone(), new_equi.clone());
        let surface = view_surface(&new_equi, &view, size, format, false)?;
        let vb = view_block(&view);
        // Komutun kendi geri alma adımına katılır (yeni adım açılmaz).
        s.edit_with("360", false, move |doc, _| {
            let l = doc.layer_mut(lid).ok_or(EngineError::NoLayer(lid))?;
            l.content = LayerContent::Raster(surface);
            set_block(l, SOURCE_KEY, bytes);
            set_block(l, VIEW_KEY, vb);
            Ok(())
        })?;
    }
    Ok(())
}

/// 360 pin rengi (turuncu, Google Haritalar işareti biçiminde) ve içteki koyu nokta.
pub const PIN_COLOR: [u8; 3] = [245, 124, 0];
const PIN_DARK: [u8; 3] = [150, 64, 0];

/// Pin görseli (RGBA8): yükseklik `h`, ucu altta ortada. Dönen: (genişlik, yükseklik, pikseller).
pub fn pin_image(h: u32) -> (u32, u32, Vec<u8>) {
    let h = h.clamp(8, 4096);
    let r = f64::from(h) * 0.36;
    let w = (r * 2.0).ceil() as u32 + 2;
    let cx = f64::from(w) / 2.0;
    let cy = r + 1.0;
    let tip = f64::from(h) - 0.5;
    let d = tip - cy;
    // Uçtan çembere teğet noktaların yüksekliği ve yarı genişliği.
    let ty = cy + r * r / d;
    let half = (r * r - (ty - cy).powi(2)).max(0.0).sqrt();
    let inside = |x: f64, y: f64| -> u8 {
        let dc = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        if dc <= r * 0.38 {
            return 2;
        }
        if dc <= r {
            return 1;
        }
        if y >= ty && y <= tip && (x - cx).abs() <= half * (tip - y) / (tip - ty) {
            return 1;
        }
        0
    };
    let n = 4u32;
    let mut out = vec![0u8; (w * h * 4) as usize];
    for py in 0..h {
        for px in 0..w {
            let (mut cov, mut inner) = (0u32, 0u32);
            for sy in 0..n {
                for sx in 0..n {
                    let x = f64::from(px) + (f64::from(sx) + 0.5) / f64::from(n);
                    let y = f64::from(py) + (f64::from(sy) + 0.5) / f64::from(n);
                    match inside(x, y) {
                        1 => cov += 1,
                        2 => {
                            cov += 1;
                            inner += 1;
                        }
                        _ => {}
                    }
                }
            }
            if cov == 0 {
                continue;
            }
            let i = ((py * w + px) * 4) as usize;
            let fi = inner as f32 / cov as f32;
            for c in 0..3 {
                out[i + c] = (f32::from(PIN_COLOR[c]) * (1.0 - fi) + f32::from(PIN_DARK[c]) * fi).round() as u8;
            }
            out[i + 3] = (cov * 255 / (n * n)) as u8;
        }
    }
    (w, h, out)
}

/// 360 pini: `{"x":px?,"y":px?,"size":px?}` → en üste yeni katman "Pin", ucu (x, y) noktasında.
fn pin(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let size = st.doc.size;
    let format = st.doc.pixel_format();
    if format.mode != ColorMode::Rgb {
        return Err(bad(PIN, crate::evren_ai_cmds::t("a pin can be added only in RGB documents", "pin yalnız RGB belgelerde eklenir")));
    }
    let x = p.get("x").and_then(Value::as_f64).unwrap_or(f64::from(size.width) / 2.0);
    let y = p.get("y").and_then(Value::as_f64).unwrap_or(f64::from(size.height) / 2.0);
    if !x.is_finite() || !y.is_finite() {
        return Err(bad(PIN, "x / y must be numbers"));
    }
    let hpx = p.get("size").and_then(Value::as_f64).filter(|v| v.is_finite() && *v >= 8.0).unwrap_or(f64::from(size.height) * 0.09).clamp(8.0, 4096.0) as u32;
    let (w, h, img) = pin_image(hpx);
    let left = (x - f64::from(w) / 2.0).round().clamp(-1e8, 1e8) as i32;
    let top = (y - f64::from(h)).round().clamp(-1e8, 1e8) as i32;
    let mut surface = Surface::from_interleaved(PixelFormat::RGBA8, Rect::from_xywh(left, top, w, h), &img);
    if format != PixelFormat::RGBA8 {
        surface = surface.try_convert(format).map_err(|err| EngineError::Other(format!("{}: {err}", crate::evren_ai_cmds::t("out of memory", "bellek yetmedi"))))?;
    }
    let layer = Layer::new("Pin", LayerContent::Raster(surface));
    let lid = layer.id;
    s.edit(crate::evren_ai_cmds::t("Add Pin", "Pin Ekle"), move |doc, active| {
        doc.layers.push(layer);
        *active = Some(lid);
        Ok(())
    })?;
    Ok(json!({"layer": lid.0}))
}

/// Katmanın 360 bilgisi (360 değilse `pano: false`).
fn info(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, INFO, p)?;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let l = st.doc.layer(id).ok_or(EngineError::NoLayer(id))?;
    Ok(match view_of(l) {
        Some(v) => json!({"layer": id.0, "pano": true, "view": v.to_json()}),
        None => json!({"layer": id.0, "pano": false, "blocks": l.psd_blocks.iter().map(|(k, d)| format!("{}:{}", String::from_utf8_lossy(k), d.len())).collect::<Vec<_>>()}),
    })
}

/// 360'ı kapat: düz panorama eski yerine geri konur, 360 bilgisi silinir.
fn off(s: &mut Session, p: &Value) -> Result<Value> {
    let id = target(s, OFF, p)?;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let l = st.doc.layer(id).ok_or(EngineError::NoLayer(id))?;
    let view = view_of(l).ok_or_else(|| bad(OFF, crate::evren_ai_cmds::t("the layer is not a 360 layer", "katman 360 değil")))?;
    let LayerContent::Raster(surface) = &l.content else {
        return Err(bad(OFF, crate::evren_ai_cmds::t("the 360 layer is not a pixel layer", "360 katmanı piksel katmanı değil")));
    };
    let format = surface.format();
    let source = l.psd_blocks.iter().find(|(k, _)| *k == SOURCE_KEY).map(|(_, d)| d.clone()).ok_or_else(|| bad(OFF, crate::evren_ai_cmds::t("360 image not found", "360 görsel bulunamadı")))?;
    let equi = decode_source(&source)?;
    let bytes: Vec<u8> = equi.px.iter().flatten().copied().collect();
    let flat = Surface::from_interleaved(PixelFormat::RGBA8, Rect::from_xywh(view.x, view.y, equi.w, equi.h), &bytes);
    let flat = if format == PixelFormat::RGBA8 { flat } else { flat.try_convert(format).map_err(|e| EngineError::Other(format!("{}: {e}", crate::evren_ai_cmds::t("out of memory", "bellek yetmedi"))))? };
    s.edit(crate::evren_ai_cmds::t("Turn Off 360", "360'ı Kapat"), move |doc, _| {
        let l = doc.layer_mut(id).ok_or(EngineError::NoLayer(id))?;
        l.content = LayerContent::Raster(flat);
        l.psd_blocks.retain(|(k, _)| *k != SOURCE_KEY && *k != VIEW_KEY && *k != META_KEY);
        Ok(())
    })?;
    Ok(json!({"layer": id.0}))
}

fn meta_block(exif: Option<&[u8]>, xmp: Option<&str>) -> Option<Arc<Vec<u8>>> {
    if exif.is_none() && xmp.is_none() {
        return None;
    }
    let exif = exif.unwrap_or_default();
    let len = u32::try_from(exif.len()).ok()?;
    let mut out = Vec::with_capacity(4 + exif.len() + xmp.map_or(0, str::len));
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(exif);
    out.extend_from_slice(xmp.unwrap_or_default().as_bytes());
    Some(Arc::new(out))
}

fn read_meta_block(data: &[u8]) -> (Option<Vec<u8>>, Option<String>) {
    let Some(len) = data.get(..4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(|b| u32::from_be_bytes(b) as usize) else { return (None, None) };
    let exif = data.get(4..4usize.saturating_add(len)).filter(|e| !e.is_empty()).map(<[u8]>::to_vec);
    let rest = data.get(4usize.saturating_add(len)..).unwrap_or_default();
    let end = rest.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
    let xmp = std::str::from_utf8(rest.get(..end).unwrap_or_default()).ok().filter(|x| !x.trim().is_empty()).map(str::to_string);
    (exif, xmp)
}

/// Google'ın 360 fotoğraf işareti (GPano, DJI'nin yazdığı biçim): görüntüleyiciler (Google Foto,
/// Facebook, Windows) bununla küre olarak açar.
fn gpano_xmp(w: u32, h: u32) -> String {
    format!(
        concat!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#,
            r#"<rdf:Description rdf:about="" xmlns:GPano="http://ns.google.com/photos/1.0/panorama/" GPano:UsePanoramaViewer="True" "#,
            r#"GPano:ProjectionType="equirectangular" GPano:CroppedAreaImageWidthPixels="{w}" GPano:CroppedAreaImageHeightPixels="{h}" "#,
            r#"GPano:FullPanoWidthPixels="{w}" GPano:FullPanoHeightPixels="{h}" GPano:CroppedAreaLeftPixels="0" GPano:CroppedAreaTopPixels="0" "#,
            r#"GPano:PoseHeadingDegrees="0"/></rdf:RDF></x:xmpmeta>"#
        ),
        w = w,
        h = h
    )
}

/// İhracın hedefi: `layer` verilmişse o, etkin katman 360 ise o, değilse belgedeki ilk 360 katmanı.
fn export_target(s: &Session, p: &Value) -> Result<LayerId> {
    let st = s.active().ok_or(EngineError::NoDocument)?;
    if p.get("layer").is_some_and(|v| !v.is_null()) {
        return target(s, EXPORT, p);
    }
    if let Some(id) = st.active_layer.filter(|id| st.doc.layer(*id).is_some_and(is_pano)) {
        return Ok(id);
    }
    st.doc.walk().into_iter().find(|(_, _, l)| is_pano(l)).map(|(_, _, l)| l.id).ok_or_else(|| bad(EXPORT, crate::evren_ai_cmds::t("the document has no 360 layer", "belgede 360 katmanı yok")))
}

fn export_enabled(s: &Session) -> std::result::Result<(), String> {
    let st = s.active().ok_or(crate::evren_ai_cmds::t("no document open", "belge açık değil"))?;
    if st.doc.walk().iter().any(|(_, _, l)| is_pano(l)) { Ok(()) } else { Err(crate::evren_ai_cmds::t("the document has no 360 layer", "belgede 360 katmanı yok").into()) }
}

/// 360 Export: kürenin tamamı (düz 2:1 panorama, tam çözünürlük) dosyaya. Ekranda görünen üst
/// katmanlar (pin, yazı, …) ve 360 katmanına yapılan işlenmemiş düzenlemeler küreye işlenir; belge
/// değişmez. JPG'de fotoğrafın kendi EXIF'i (kamera, GPS) ve XMP'si (GPano) korunur, yoksa GPano
/// eklenir: DJI Mini 4 Pro'nun 360 fotoğrafı gibi küre olarak açılır.
fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let path = p.get("path").and_then(Value::as_str).filter(|v| !v.trim().is_empty()).ok_or_else(|| bad(EXPORT, "`path` (kayıt yeri) gerekli"))?.to_string();
    let quality = p.get("quality").and_then(Value::as_u64).unwrap_or(95).clamp(1, 100) as u8;
    let id = export_target(s, p)?;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let size = st.doc.size;
    let l = st.doc.layer(id).ok_or(EngineError::NoLayer(id))?;
    let view = view_of(l).ok_or_else(|| bad(EXPORT, crate::evren_ai_cmds::t("the layer is not a 360 layer", "katman 360 değil")))?;
    let source = source_of(l).ok_or_else(|| bad(EXPORT, crate::evren_ai_cmds::t("360 image not found", "360 görsel bulunamadı")))?;
    let (exif, mut xmp) = l.psd_blocks.iter().find(|(k, _)| *k == META_KEY).map(|(_, d)| read_meta_block(d)).unwrap_or_else(|| {
        let m = &st.doc.metadata;
        if m.xmp.as_deref().is_some_and(|x| x.contains("GPano")) { (m.exif.as_deref().cloned(), m.xmp.as_deref().map(str::to_string)) } else { (None, None) }
    });
    let equi = decode_source(&source)?;
    // Ekranda görünen hâl (360 katmanı + görünen üst katmanlar) küreye işlenir.
    let shown = if l.visible {
        let flat = crate::pixels::composite_layers(&st.doc, PixelFormat::RGBA8, None);
        Some(read_view(&flat, size))
    } else {
        None
    };
    let sphere = match shown.as_deref().map(|v| bake(&equi, &view, v, size)).transpose()?.flatten() {
        Some(e) => Arc::new(e),
        None => equi,
    };
    let ext = std::path::Path::new(&path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_else(|| "jpg".into());
    let rgb: Vec<u8> = sphere.px.iter().flat_map(|c| [c[0], c[1], c[2]]).collect();
    let bytes = match ext.as_str() {
        "jpg" | "jpeg" => {
            if !xmp.as_deref().is_some_and(|x| x.contains("GPano")) {
                xmp = Some(gpano_xmp(sphere.w, sphere.h));
            }
            photocraft_codecs::web::encode_jpeg_rgb8(sphere.w, sphere.h, &rgb, quality, false, true, None, None, exif.as_deref(), xmp.as_deref())
        }
        "png" | "tif" | "tiff" => {
            let format = if ext == "png" { photocraft_codecs::Format::Png } else { photocraft_codecs::Format::Tiff };
            photocraft_codecs::Image::from_u8(sphere.w, sphere.h, photocraft_codecs::ChannelLayout::Rgb, rgb)
                .and_then(|img| photocraft_codecs::encode(&img, format, &photocraft_codecs::EncodeOptions::default()))
        }
        other => return Err(bad(EXPORT, format!("`.{other}`: {}", crate::evren_ai_cmds::t("not supported; use .jpg, .png or .tif", "desteklenmiyor: .jpg, .png ya da .tif")))),
    }
    .map_err(|e| EngineError::Other(format!("{}: {e}", crate::evren_ai_cmds::t("could not encode the 360 image", "360 görsel kodlanamadı"))))?;
    let tmp = format!("{path}.evren-tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| EngineError::Other(format!("{}: {e}", crate::evren_ai_cmds::t("could not write", "yazılamadı"))))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        EngineError::Other(format!("{}: {e}", crate::evren_ai_cmds::t("could not write", "yazılamadı")))
    })?;
    Ok(json!({"path": path, "width": sphere.w, "height": sphere.h, "bytes": bytes.len(), "gpano": ext.starts_with("jp"), "exif": exif.is_some()}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: EXPORT,
            label: "360 Export…",
            menu: &["File", "Export"],
            shortcut: None,
            params: r#"{"path":string,"layer":id?,"quality":1..100=95} → {"path","width","height","bytes","gpano","exif"} (EVREN: the whole 360 sphere with the visible layers above baked in; .jpg (EXIF + GPano), .png, .tif)"#,
            enabled: export_enabled,
            run: export,
            journal: false,
        },
        CommandSpec {
            id: MAKE,
            label: "Make 360",
            menu: &[],
            shortcut: None,
            params: r#"{"layer":id?,"yaw":deg?,"pitch":deg?,"roll":deg?,"fov":deg?} → {"layer","view","width","height","ratio"} (EVREN: keeps the layer's flat 360 panorama and turns the layer into a perspective view)"#,
            enabled: make_enabled,
            run: make,
            journal: true,
        },
        CommandSpec {
            id: VIEW,
            label: "360 View",
            menu: &[],
            shortcut: None,
            params: r#"{"layer":id?,"yaw":deg?,"pitch":deg?,"roll":deg?,"fov":deg?,"fast":bool?} → {"layer","view"} (EVREN: turns a 360 layer; yaw -180..180, pitch ±90, roll ±45, fov 10..150; fast = quick draw while dragging)"#,
            enabled: pano_enabled,
            run: view_cmd,
            journal: true,
        },
        CommandSpec {
            id: INFO,
            label: "360 Info",
            menu: &[],
            shortcut: None,
            params: r#"{"layer":id?} → {"layer","pano":bool,"view"?} (EVREN: whether the layer is 360, and its view)"#,
            enabled: info_enabled,
            run: info,
            journal: false,
        },
        CommandSpec {
            id: PIN,
            label: "Add Pin",
            menu: &[],
            shortcut: None,
            params: r#"{"x":px?,"y":px?,"size":px?} → {"layer"} (EVREN: an orange map pin layer on top, its tip at x,y; default height 9% of the document)"#,
            enabled: rgb_doc,
            run: pin,
            journal: true,
        },
        CommandSpec {
            id: OFF,
            label: "Turn Off 360",
            menu: &[],
            shortcut: None,
            params: r#"{"layer":id?} → {"layer"} (EVREN: puts the flat panorama back)"#,
            enabled: pano_enabled,
            run: off,
            journal: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn equi() -> Equi {
        // 8×4: sol yarı kırmızı, sağ yarı mavi.
        let (w, h) = (8u32, 4u32);
        let px = (0..w * h).map(|i| if i % w < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        Equi { w, h, px }
    }

    #[test]
    fn yaw_turns_the_view() {
        let e = equi();
        // yaw 0: merkez boylam 0 = görselin ortası; yaw -90 = sol (kırmızı), +90 = sağ (mavi).
        let left = render(&e, &View { yaw: -90.0, fov: 30.0, ..View::default() }, 4, 2, true);
        let right = render(&e, &View { yaw: 90.0, fov: 30.0, ..View::default() }, 4, 2, true);
        assert!(left[0] > 200 && left[2] < 50);
        assert!(right[2] > 200 && right[0] < 50);
    }

    #[test]
    fn clamps_hostile_values() {
        let v = View { yaw: 540.0, pitch: f64::NAN, roll: 1e9, fov: -5.0, ..View::default() }.clamped();
        assert!((v.yaw - 180.0).abs() < 1e-9 || (v.yaw + 180.0).abs() < 1e-9);
        assert_eq!(v.pitch, 0.0);
        assert_eq!(v.roll, ROLL_MAX);
        assert_eq!(v.fov, FOV_MIN);
    }

    #[test]
    fn view_json_round_trips() {
        let v = View { yaw: 12.5, pitch: -3.0, roll: 1.0, fov: 70.0, x: 4, y: -2 };
        assert_eq!(View::from_json(&v.to_json()), Some(v));
        assert_eq!(View::from_json(&json!(5)), None);
    }
}
