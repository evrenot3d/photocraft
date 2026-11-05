//! EVREN eklentisi: yapay zekâ araçlarının (Upscale, Alan derinliği, Bitki / Cam ayır, Gökyüzü, Gürültü temizleme) belge tarafı.
//! Yapay zekâ işi PhotoCraft'ın dışında, `evren_ai\sunucu.py` (Python + ComfyUI) sürecinde yapılır; bu komutlar
//! belgenin görünen hâlini PNG'ye yazar ve sonucu yeni katman olarak geri koyar.

use photocraft_color::PixelFormat;
use photocraft_doc::{Layer, LayerContent};
use photocraft_geom::Rect;
use photocraft_raster::Surface;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

/// EVREN eklentilerinin dili: arayüz her karede ayarlar (ui-egui/src/evren/dil.rs). Türkçe değilse İngilizce.
static DIL_TR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn dil_ayarla(turkce: bool) {
    DIL_TR.store(turkce, std::sync::atomic::Ordering::Relaxed);
}

pub fn turkce() -> bool {
    DIL_TR.load(std::sync::atomic::Ordering::Relaxed)
}

/// `t("English", "Türkçe")`
pub fn t(en: &'static str, tr: &'static str) -> &'static str {
    if turkce() { tr } else { en }
}

pub const COMPOSITE: &str = "evren.ai.composite";
pub const ADD_LAYER: &str = "evren.ai.addLayer";
pub const UPSCALE: &str = "evren.ai.upscaleApply";

fn bad(cmd: &str, message: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: message.into() }
}

fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| t("no document open", "belge açık değil").into())
}

fn path_param(cmd: &str, p: &Value) -> Result<String> {
    p.get("path").and_then(Value::as_str).filter(|v| !v.trim().is_empty()).map(str::to_string).ok_or_else(|| bad(cmd, "`path` is required"))
}

/// Belgenin görünen hâli (bütün görünen katmanlar), RGBA8. `white`: saydam yerler beyaz.
fn composite_rgba(s: &Session, white: bool) -> Result<(u32, u32, Vec<u8>)> {
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let (w, h) = (st.doc.size.width, st.doc.size.height);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > (1 << 28) {
        return Err(EngineError::Other(t("invalid document size", "belge boyutu geçersiz").into()));
    }
    let flat = crate::pixels::composite_layers(&st.doc, PixelFormat::RGBA8, if white { Some([1.0, 1.0, 1.0]) } else { None });
    let mut px = vec![[0u8; 4]; w as usize * h as usize];
    flat.read_rgba8_into(Rect::from_xywh(0, 0, w, h), &mut px);
    if white {
        for p in &mut px {
            let a = u32::from(p[3]);
            for c in 0..3 {
                p[c] = ((u32::from(p[c]) * a + 255 * (255 - a)) / 255) as u8;
            }
            p[3] = 255;
        }
    }
    Ok((w, h, px.into_iter().flatten().collect()))
}

/// `{"path":string,"white":bool?}` → belgenin görünen hâli PNG olarak.
fn composite(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(COMPOSITE, p)?;
    let white = p.get("white").and_then(Value::as_bool).unwrap_or(false);
    let (w, h, rgba) = composite_rgba(s, white)?;
    let img = photocraft_codecs::Image::from_u8(w, h, photocraft_codecs::ChannelLayout::Rgba, rgba).map_err(|e| EngineError::Other(e.to_string()))?;
    let mut opts = photocraft_codecs::EncodeOptions::default();
    opts.png_compression = photocraft_codecs::PngCompression::Fast;
    let bytes = photocraft_codecs::encode(&img, photocraft_codecs::Format::Png, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, bytes).map_err(|e| EngineError::Other(format!("{}: {e}", t("could not write", "yazılamadı"))))?;
    Ok(json!({"path": path, "width": w, "height": h}))
}

/// PNG dosyasından belge biçiminde yüzey (sol üst köşe 0,0).
fn read_surface(cmd: &str, path: &str, format: PixelFormat) -> Result<(u32, u32, Surface)> {
    let bytes = std::fs::read(path).map_err(|e| bad(cmd, format!("{}: {e}", t("could not read", "okunamadı"))))?;
    let img = photocraft_codecs::decode(&bytes).map_err(|e| bad(cmd, format!("{}: {e}", t("could not decode", "çözülemedi"))))?;
    let (w, h) = img.dimensions();
    let rgba = img.to_rgba8();
    let surface = Surface::from_interleaved(PixelFormat::RGBA8, Rect::from_xywh(0, 0, w, h), &rgba);
    let surface = if format == PixelFormat::RGBA8 { surface } else { surface.try_convert(format).map_err(|e| EngineError::Other(format!("{}: {e}", t("out of memory", "bellek yetmedi"))))? };
    Ok((w, h, surface))
}

fn push_layer(s: &mut Session, label: &str, name: &str, surface: Surface) -> Result<u64> {
    let layer = Layer::new(name, LayerContent::Raster(surface));
    let lid = layer.id;
    s.edit(label, move |doc, active| {
        doc.layers.push(layer);
        *active = Some(lid);
        Ok(())
    })?;
    Ok(lid.0)
}

/// `{"path":string,"name":string?}` → en üste yeni katman (PNG, sol üst köşe belgenin köşesinde).
fn add_layer(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(ADD_LAYER, p)?;
    let name = p.get("name").and_then(Value::as_str).unwrap_or(t("Layer", "Katman")).to_string();
    let format = s.active().ok_or(EngineError::NoDocument)?.doc.pixel_format();
    let (_, _, surface) = read_surface(ADD_LAYER, &path, format)?;
    let id = push_layer(s, &name, &name, surface)?;
    Ok(json!({"layer": id}))
}

/// Upscale sonucu: `{"path":string,"name":string?}` → belge (bütün katmanlar, Lanczos) sonucun boyutuna büyür,
/// sonuç en üste yeni katman olur. Tek geri alma adımı.
fn upscale_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(UPSCALE, p)?;
    let name = p.get("name").and_then(Value::as_str).unwrap_or("Upscale").to_string();
    let format = s.active().ok_or(EngineError::NoDocument)?.doc.pixel_format();
    let (w, h, surface) = read_surface(UPSCALE, &path, format)?;
    let key = format!("evren-upscale-{}", photocraft_doc::LayerId::fresh().0);
    s.execute("image.imageSize", json!({"width": w, "height": h, "resample": "lanczos", "coalesce": key}))?;
    let layer = Layer::new(name.clone(), LayerContent::Raster(surface));
    let lid = layer.id;
    s.coalesce_request = Some(key);
    let r = s.edit("Upscale", move |doc, active| {
        doc.layers.push(layer);
        *active = Some(lid);
        Ok(())
    });
    s.coalesce_request = None;
    r?;
    Ok(json!({"layer": lid.0, "width": w, "height": h}))
}

// ------------------------------------------------------------------ Gürültü temizleme (Intel Open Image Denoise)
//
// Temizleme işini `evren_ai\oidn\bin\evren_oidn.exe` yapar (arayüz tarafı: ui-egui/src/evren/gurultu_ui.rs).
// Sonuç YENİ KATMAN AÇILMADAN seçili katmanın kendi piksellerine yazılır. Katman kendi biçiminde (8/16/32 bit)
// okunur ve aynı bölgeye geri yazılır; bölge dışı, saydamlık, maske, efektler dokunulmadan kalır. Pencere açıkken
// yapılan bütün değişiklikler (miktar, keskinlik, önizleme aç/kapa, yeni sonuç) aynı `coalesce` anahtarıyla tek
// geri alma adımında birleşir.

pub const DENOISE_APPLY: &str = "evren.denoise.apply";

/// Arayüzün hazırladığı katman pikselleri (bölge, katmanın kanal düzeninde 0–1 değerler): JSON'a sığmaz.
static BEKLEYEN_PIKSEL: std::sync::Mutex<Option<(Rect, Vec<f32>)>> = std::sync::Mutex::new(None);

/// Sonraki `evren.denoise.apply` komutunun pikselleri.
pub fn piksel_birak(bolge: Rect, veri: Vec<f32>) {
    if let Ok(mut b) = BEKLEYEN_PIKSEL.lock() {
        *b = Some((bolge, veri));
    }
}

/// Temizlenecek katmanın yüzeyi (yalnız RGB, kilitsiz, resim katmanı).
fn denoise_surface<'a>(doc: &'a photocraft_doc::Document, id: photocraft_doc::LayerId) -> std::result::Result<&'a Surface, String> {
    let l = doc.layer(id).ok_or(t("selected layer not found", "seçili katman bulunamadı"))?;
    if l.locks.all || l.locks.pixels {
        return Err(if turkce() { format!("\"{}\" katmanının pikselleri kilitli", l.name) } else { format!("the pixels of \"{}\" are locked", l.name) });
    }
    let LayerContent::Raster(sf) = &l.content else {
        return Err(if turkce() { format!("\"{}\" bir resim katmanı değil (metin, şekil, grup, akıllı nesne ya da ayar katmanı); önce rasterleştirin", l.name) } else { format!("\"{}\" is not a pixel layer (type, shape, group, smart object or adjustment layer); rasterize it first", l.name) });
    };
    if sf.format().mode != photocraft_doc::ColorMode::Rgb {
        return Err(t("Denoise works only in RGB documents", "Gürültü temizleme yalnız RGB belgelerde çalışır").into());
    }
    Ok(sf)
}

/// Temizlenecek katman: (ad, bölge, kanal sayısı, bölgenin pikselleri 0–1, katmanın kanal düzeninde).
pub fn katman_kaynak(s: &Session, katman: u64) -> std::result::Result<(String, Rect, usize, Vec<f32>), String> {
    let st = s.active().ok_or(t("no document open", "belge açık değil"))?;
    let id = photocraft_doc::LayerId(katman);
    let sf = denoise_surface(&st.doc, id)?;
    let bolge = sf.content_bounds();
    if bolge.is_empty() {
        return Err(t("the selected layer is empty", "seçili katman boş").into());
    }
    if u64::from(bolge.width()) * u64::from(bolge.height()) > (1 << 28) {
        return Err(t("the layer is too large", "katman çok büyük").into());
    }
    let veri = sf.read_region(bolge);
    let n = veri.len() / (bolge.width() as usize * bolge.height() as usize);
    if n < 3 {
        return Err(t("could not read the layer's color channels", "katmanın renk kanalları okunamadı").into());
    }
    let ad = st.doc.layer(id).map(|l| l.name.clone()).unwrap_or_default();
    Ok((ad, bolge, n, veri))
}

/// `{"coalesce":string,"katman":u64}` + `piksel_birak` → katmanın o bölgesi bu piksellerle değişir.
fn denoise_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let key = p.get("coalesce").and_then(Value::as_str).filter(|k| !k.is_empty()).ok_or_else(|| bad(DENOISE_APPLY, "`coalesce` is required"))?.to_string();
    let id = p.get("katman").and_then(Value::as_u64).map(photocraft_doc::LayerId).ok_or_else(|| bad(DENOISE_APPLY, "`katman` is required"))?;
    let (bolge, veri) = BEKLEYEN_PIKSEL.lock().ok().and_then(|mut b| b.take()).ok_or_else(|| bad(DENOISE_APPLY, "no pending pixels"))?;
    {
        let st = s.active().ok_or(EngineError::NoDocument)?;
        let sf = denoise_surface(&st.doc, id).map_err(|e| bad(DENOISE_APPLY, e))?;
        let n = sf.format().channels();
        if veri.len() != bolge.width() as usize * bolge.height() as usize * n {
            return Err(bad(DENOISE_APPLY, t("the pixel count does not match the layer (it may have changed; reopen the window)", "piksel sayısı katmanla uyuşmuyor (katman değişmiş olabilir; pencereyi yeniden açın)")));
        }
    }
    s.coalesce_request = Some(key);
    let r = s.edit(t("Denoise", "Gürültü temizleme"), move |doc, _active| {
        let l = doc.layer_mut(id).ok_or_else(|| EngineError::Other(t("layer not found", "katman bulunamadı").into()))?;
        if let LayerContent::Raster(sf) = &mut l.content {
            sf.write_region_par(bolge, &veri);
        }
        Ok(())
    });
    s.coalesce_request = None;
    r?;
    Ok(json!({"katman": id.0}))
}

// ------------------------------------------------------------------ Çizgi (Procreate QuickShape gibi)
//
// Arayüz (ui-egui/src/evren/cizgi_ui.rs) çizgiyi / şekli kendisi pürüzsüz kenarlı bir kapsama haritasına çizer;
// bu komut o haritayı ön plan rengiyle seçili katmana boyar (seçim varsa yalnız seçimin içine). Seçili katman
// boyanamıyorsa (metin, grup, ayar katmanı, kilitli) üstüne "Çizgi" adlı yeni katman açılır. Tek geri alma adımı.

pub const CIZGI: &str = "evren.cizgi.ciz";

/// Arayüzün hazırladığı kapsama (bölge, piksel başına 0–1): JSON'a sığmaz.
static BEKLEYEN_CIZGI: std::sync::Mutex<Option<(Rect, Vec<f32>)>> = std::sync::Mutex::new(None);

/// Sonraki `evren.cizgi.ciz` komutunun kapsaması.
pub fn cizgi_birak(bolge: Rect, kapsam: Vec<f32>) {
    if let Ok(mut b) = BEKLEYEN_CIZGI.lock() {
        *b = Some((bolge, kapsam));
    }
}

fn boyanabilir(doc: &photocraft_doc::Document, id: photocraft_doc::LayerId) -> bool {
    doc.layer(id).is_some_and(|l| !l.locks.all && !l.locks.pixels && matches!(l.content, LayerContent::Raster(_)))
}

/// `{"renk":[r,g,b],"opaklik":0–1}` + `cizgi_birak` → `{"katman"}`.
fn cizgi_ciz(s: &mut Session, p: &Value) -> Result<Value> {
    let renk: Vec<f32> = p.get("renk").and_then(Value::as_array).map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
    let renk = [renk.first().copied().unwrap_or(0.0), renk.get(1).copied().unwrap_or(0.0), renk.get(2).copied().unwrap_or(0.0)];
    let opaklik = p.get("opaklik").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.0, 1.0) as f32;
    let (bolge, kapsam) = BEKLEYEN_CIZGI.lock().ok().and_then(|mut b| b.take()).ok_or_else(|| bad(CIZGI, "no pending stroke"))?;
    let npx = bolge.width() as usize * bolge.height() as usize;
    if kapsam.len() != npx || npx == 0 {
        return Err(bad(CIZGI, "inconsistent stroke size"));
    }
    let st = s.active().ok_or(EngineError::NoDocument)?;
    if st.doc.pixel_format().mode != photocraft_doc::ColorMode::Rgb {
        return Err(bad(CIZGI, t("The Line tool works only in RGB documents", "Çizgi aracı yalnız RGB belgelerde çalışır")));
    }
    // Seçim varsa yalnız içine (Fırça gibi).
    let secim: Option<Vec<f32>> = st.doc.selection.as_ref().map(|sel| {
        let v = sel.read_region(bolge);
        let n = (v.len() / npx).max(1);
        v.chunks(n).map(|c| c[0]).collect()
    });
    let hedef = st.active_layer.filter(|id| boyanabilir(&st.doc, *id));
    let r = s.edit(t("Line", "Çizgi"), move |doc, active| {
        let id = match hedef {
            Some(id) => id,
            None => {
                let l = Layer::new(t("Line", "Çizgi"), LayerContent::Raster(Surface::new(doc.pixel_format())));
                let id = doc.insert_above(*active, l);
                *active = Some(id);
                id
            }
        };
        let l = doc.layer_mut(id).ok_or_else(|| EngineError::Other(t("layer not found", "katman bulunamadı").into()))?;
        let koru = l.locks.transparency;
        if let LayerContent::Raster(sf) = &mut l.content {
            let f = sf.format();
            let n = f.channels();
            let mut v = sf.read_region(bolge);
            for i in 0..npx {
                let a = kapsam[i] * opaklik * secim.as_ref().map_or(1.0, |s| s.get(i).copied().unwrap_or(0.0));
                if a <= 0.0 {
                    continue;
                }
                let px = &mut v[i * n..i * n + n];
                if f.alpha && !koru {
                    let da = px[n - 1];
                    let oa = a + da * (1.0 - a);
                    for c in 0..3 {
                        px[c] = if oa > 0.0 { (renk[c] * a + px[c] * da * (1.0 - a)) / oa } else { 0.0 };
                    }
                    px[n - 1] = oa;
                } else {
                    for c in 0..3 {
                        px[c] += (renk[c] - px[c]) * a;
                    }
                }
            }
            sf.write_region_par(bolge, &v);
        }
        Ok(id)
    })?;
    Ok(json!({"katman": r.0}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: CIZGI,
            label: "Line",
            menu: &[],
            shortcut: None,
            params: r#"{"renk":[r,g,b],"opaklik":f64?} → {"katman"} (EVREN: paints the Line tool's shape on the selected layer; coverage via cizgi_birak)"#,
            enabled: has_doc,
            run: cizgi_ciz,
            journal: false,
        },
        CommandSpec {
            id: DENOISE_APPLY,
            label: "Apply Denoise",
            menu: &[],
            shortcut: None,
            params: r#"{"coalesce":string,"katman":u64} → {"katman"} (EVREN: writes the Denoise result into the selected layer's pixels; pixels via piksel_birak)"#,
            enabled: has_doc,
            run: denoise_apply,
            journal: false,
        },
        CommandSpec {
            id: COMPOSITE,
            label: "Write Visible Image",
            menu: &[],
            shortcut: None,
            params: r#"{"path":string,"white":bool?} → {"path","width","height"} (EVREN: the document's visible image as PNG)"#,
            enabled: has_doc,
            run: composite,
            journal: false,
        },
        CommandSpec {
            id: ADD_LAYER,
            label: "Add Image as Layer",
            menu: &[],
            shortcut: None,
            params: r#"{"path":string,"name":string?} → {"layer"} (EVREN: a PNG as a new top layer)"#,
            enabled: has_doc,
            run: add_layer,
            journal: true,
        },
        CommandSpec {
            id: UPSCALE,
            label: "Apply Upscale",
            menu: &[],
            shortcut: None,
            params: r#"{"path":string,"name":string?} → {"layer","width","height"} (EVREN: the document grows to the result's size, the result becomes the top layer)"#,
            enabled: has_doc,
            run: upscale_apply,
            journal: true,
        },
    ]
}
