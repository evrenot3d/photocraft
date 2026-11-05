//! evren_oidn: PhotoCraft'ın "Gürültü temizleme" aracı için Intel Open Image Denoise (OIDN 2.x) yardımcısı.
//!
//! PhotoCraft'ın kendi kodunda `unsafe` yasak (çalışma alanı kuralı), OIDN ise bir C kütüphanesi. Bu yüzden
//! OIDN bu küçük ayrı programdan kullanılır. Program yalnız Gürültü temizleme penceresi açıkken çalışır: ekran
//! kartı (CUDA) hazırda bekler, ayar değişince yeniden temizler; pencere kapanınca stdin kapanır ve çıkar.
//!
//! Konuşma (stdin → stdout, ikili güvenli):
//!   `BILGI <cihaz>\n`                                → `OK 0 <cihaz adı>\n`
//!   `TEMIZLE <w> <h> <kalite> <cihaz>\n` + w·h·3 bayt RGB8 (sRGB)
//!                                                    → `OK <ms> <cihaz adı>\n` + w·h·3 bayt RGB8
//!   hata                                             → `HATA <mesaj>\n`
//! cihaz: 0 otomatik (ekran kartı varsa o), 1 işlemci, 3 CUDA (NVIDIA). kalite: 4 hızlı, 5 dengeli, 6 yüksek.

use std::ffi::{CStr, CString, c_char, c_void};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::time::Instant;

type Device = *mut c_void;
type Filter = *mut c_void;
type Buffer = *mut c_void;

const FORMAT_FLOAT3: i32 = 3;

#[link(name = "OpenImageDenoise", kind = "raw-dylib")]
unsafe extern "C" {
    fn oidnNewDevice(device_type: i32) -> Device;
    fn oidnCommitDevice(device: Device);
    fn oidnReleaseDevice(device: Device);
    fn oidnGetDeviceError(device: Device, message: *mut *const c_char) -> i32;
    fn oidnGetDeviceInt(device: Device, name: *const c_char) -> i32;
    fn oidnGetNumPhysicalDevices() -> i32;
    fn oidnGetPhysicalDeviceInt(id: i32, name: *const c_char) -> i32;
    fn oidnGetPhysicalDeviceString(id: i32, name: *const c_char) -> *const c_char;
    fn oidnNewBuffer(device: Device, byte_size: usize) -> Buffer;
    fn oidnReleaseBuffer(buffer: Buffer);
    fn oidnWriteBuffer(buffer: Buffer, byte_offset: usize, byte_size: usize, src: *const c_void);
    fn oidnReadBuffer(buffer: Buffer, byte_offset: usize, byte_size: usize, dst: *mut c_void);
    fn oidnNewFilter(device: Device, filter_type: *const c_char) -> Filter;
    fn oidnReleaseFilter(filter: Filter);
    #[allow(clippy::too_many_arguments)]
    fn oidnSetFilterImage(filter: Filter, name: *const c_char, buffer: Buffer, format: i32, width: usize, height: usize, byte_offset: usize, pixel_stride: usize, row_stride: usize);
    fn oidnSetFilterBool(filter: Filter, name: *const c_char, value: bool);
    fn oidnSetFilterInt(filter: Filter, name: *const c_char, value: i32);
    fn oidnCommitFilter(filter: Filter);
    fn oidnExecuteFilter(filter: Filter);
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

/// Cihazın son hatası (yoksa Ok).
fn denetle(device: Device, adim: &str) -> Result<(), String> {
    let mut msg: *const c_char = std::ptr::null();
    let kod = unsafe { oidnGetDeviceError(device, &mut msg) };
    if kod == 0 {
        return Ok(());
    }
    let metin = if msg.is_null() { String::new() } else { unsafe { CStr::from_ptr(msg) }.to_string_lossy().into_owned() };
    Err(format!("{adim}: {metin} (kod {kod})"))
}

struct Cihaz {
    istek: i32,
    device: Device,
    ad: String,
}

impl Drop for Cihaz {
    fn drop(&mut self) {
        unsafe { oidnReleaseDevice(self.device) };
    }
}

fn tur_adi(t: i32) -> &'static str {
    match t {
        1 => "Processor (CPU)",
        2 => "SYCL",
        3 => "Graphics card (CUDA)",
        4 => "Graphics card (HIP)",
        5 => "Metal",
        _ => "?",
    }
}

fn cihaz_ac(istek: i32) -> Result<Cihaz, String> {
    let device = unsafe { oidnNewDevice(istek) };
    if device.is_null() {
        denetle(std::ptr::null_mut(), "could not open the device")?;
        return Err(if istek == 3 { "the graphics card (CUDA) cannot be used".into() } else { "could not open the device".into() });
    }
    unsafe { oidnCommitDevice(device) };
    if let Err(e) = denetle(device, "could not prepare the device") {
        unsafe { oidnReleaseDevice(device) };
        return Err(e);
    }
    let tur = unsafe { oidnGetDeviceInt(device, c("type").as_ptr()) };
    // Aynı türdeki ilk fiziksel cihazın adı (ör. "NVIDIA GeForce RTX 5060 Laptop GPU").
    let mut ad = String::new();
    let n = unsafe { oidnGetNumPhysicalDevices() };
    for i in 0..n {
        if unsafe { oidnGetPhysicalDeviceInt(i, c("type").as_ptr()) } == tur {
            let p = unsafe { oidnGetPhysicalDeviceString(i, c("name").as_ptr()) };
            if !p.is_null() {
                ad = unsafe { CStr::from_ptr(p) }.to_string_lossy().trim().to_string();
            }
            break;
        }
    }
    let ad = if ad.is_empty() { tur_adi(tur).to_string() } else { format!("{} · {ad}", tur_adi(tur)) };
    Ok(Cihaz { istek, device, ad })
}

struct Suzgec {
    w: usize,
    h: usize,
    kalite: i32,
    filter: Filter,
    buffer: Buffer,
}

impl Drop for Suzgec {
    fn drop(&mut self) {
        unsafe {
            oidnReleaseFilter(self.filter);
            oidnReleaseBuffer(self.buffer);
        }
    }
}

fn suzgec_ac(cihaz: &Cihaz, w: usize, h: usize, kalite: i32) -> Result<Suzgec, String> {
    let d = cihaz.device;
    let boyut = w.checked_mul(h).and_then(|n| n.checked_mul(12)).ok_or("image too large")?;
    let buffer = unsafe { oidnNewBuffer(d, boyut) };
    if buffer.is_null() {
        denetle(d, "out of memory")?;
        return Err("out of memory".into());
    }
    let filter = unsafe { oidnNewFilter(d, c("RT").as_ptr()) };
    if filter.is_null() {
        unsafe { oidnReleaseBuffer(buffer) };
        denetle(d, "could not create the filter")?;
        return Err("could not create the filter".into());
    }
    let s = Suzgec { w, h, kalite, filter, buffer };
    unsafe {
        // Girdi ve çıktı aynı bellek (yerinde temizleme, OIDN destekler): yarı bellek.
        oidnSetFilterImage(filter, c("color").as_ptr(), buffer, FORMAT_FLOAT3, w, h, 0, 0, 0);
        oidnSetFilterImage(filter, c("output").as_ptr(), buffer, FORMAT_FLOAT3, w, h, 0, 0, 0);
        // 8 bit resim: sRGB eğrili, HDR değil.
        oidnSetFilterBool(filter, c("hdr").as_ptr(), false);
        oidnSetFilterBool(filter, c("srgb").as_ptr(), true);
        oidnSetFilterInt(filter, c("quality").as_ptr(), kalite);
        oidnCommitFilter(filter);
    }
    denetle(d, "could not prepare the filter")?;
    Ok(s)
}

struct Durum {
    cihaz: Option<Cihaz>,
    suzgec: Option<Suzgec>,
}

impl Durum {
    fn cihaz(&mut self, istek: i32) -> Result<&Cihaz, String> {
        if self.cihaz.as_ref().is_none_or(|c| c.istek != istek) {
            // Önce süzgeç (eski cihaza ait) bırakılır.
            self.suzgec = None;
            self.cihaz = None;
            self.cihaz = Some(cihaz_ac(istek)?);
        }
        self.cihaz.as_ref().ok_or_else(|| "no device".to_string())
    }

    fn temizle(&mut self, w: usize, h: usize, kalite: i32, istek: i32, rgb: &[u8]) -> Result<(Vec<u8>, String), String> {
        self.cihaz(istek)?;
        let cihaz = self.cihaz.as_ref().ok_or("no device")?;
        if self.suzgec.as_ref().is_none_or(|s| (s.w, s.h, s.kalite) != (w, h, kalite)) {
            self.suzgec = None;
            self.suzgec = Some(suzgec_ac(cihaz, w, h, kalite)?);
        }
        let s = self.suzgec.as_ref().ok_or("no filter")?;
        let n = w * h * 3;
        let mut f: Vec<f32> = rgb.iter().map(|&v| f32::from(v) / 255.0).collect();
        unsafe {
            oidnWriteBuffer(s.buffer, 0, n * 4, f.as_ptr().cast());
        }
        denetle(cihaz.device, "could not upload the image")?;
        unsafe { oidnExecuteFilter(s.filter) };
        denetle(cihaz.device, "denoising failed")?;
        unsafe {
            oidnReadBuffer(s.buffer, 0, n * 4, f.as_mut_ptr().cast());
        }
        denetle(cihaz.device, "could not read the result")?;
        let cikti = f.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect();
        Ok((cikti, cihaz.ad.clone()))
    }
}

fn yanit_hata(out: &mut impl Write, e: &str) -> std::io::Result<()> {
    writeln!(out, "HATA {}", e.replace(['\n', '\r'], " "))?;
    out.flush()
}

fn main() {
    let stdin = std::io::stdin();
    let mut girdi = BufReader::with_capacity(1 << 20, stdin.lock());
    let stdout = std::io::stdout();
    let mut out = BufWriter::with_capacity(1 << 20, stdout.lock());
    let mut durum = Durum { cihaz: None, suzgec: None };
    let mut satir = String::new();
    loop {
        satir.clear();
        match girdi.read_line(&mut satir) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let p: Vec<&str> = satir.split_whitespace().collect();
        let sonuc = match p.as_slice() {
            ["BILGI", cihaz] => {
                let istek = cihaz.parse().unwrap_or(0);
                match durum.cihaz(istek) {
                    Ok(c) => writeln!(out, "OK 0 {}", c.ad).and_then(|_| out.flush()),
                    Err(e) => yanit_hata(&mut out, &e),
                }
            }
            ["TEMIZLE", w, h, kalite, cihaz] => {
                let (w, h): (usize, usize) = (w.parse().unwrap_or(0), h.parse().unwrap_or(0));
                let n = w.saturating_mul(h).saturating_mul(3);
                if w == 0 || h == 0 || n > 3 * 400_000_000 {
                    yanit_hata(&mut out, "invalid image size")
                } else {
                    let mut rgb = vec![0u8; n];
                    if girdi.read_exact(&mut rgb).is_err() {
                        break;
                    }
                    let t = Instant::now();
                    match durum.temizle(w, h, kalite.parse().unwrap_or(6), cihaz.parse().unwrap_or(0), &rgb) {
                        Ok((cikti, ad)) => {
                            let ms = t.elapsed().as_millis();
                            writeln!(out, "OK {ms} {ad}").and_then(|_| out.write_all(&cikti)).and_then(|_| out.flush())
                        }
                        Err(e) => yanit_hata(&mut out, &e),
                    }
                }
            }
            _ => yanit_hata(&mut out, "unknown request"),
        };
        if sonuc.is_err() {
            break;
        }
    }
}
