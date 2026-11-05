"""Alan derinliği ve gökyüzü değiştirme (OpenCV + numpy)."""
import cv2
import numpy as np


# ------------------------------------------------------------------ yardımcılar
def _onceki(rgba):
    """uint8 RGBA → (ön-çarpımlı float RGB, alfa) — bulanıklıkta saydam kenar koyulaşmasın."""
    a = rgba[..., 3:4].astype(np.float32) / 255.0
    return rgba[..., :3].astype(np.float32) * a, a


def _sonraki(rgb_p, a, rgba):
    out = np.empty_like(rgba)
    with np.errstate(divide="ignore", invalid="ignore"):
        rgb = np.where(a > 1e-4, rgb_p / np.maximum(a, 1e-4), 0)
    out[..., :3] = np.clip(rgb + 0.5, 0, 255).astype(np.uint8)
    out[..., 3] = np.clip(a[..., 0] * 255 + 0.5, 0, 255).astype(np.uint8)
    return out


def _uygula(rgba, fn):
    """fn(float görüntü) her kanala (ön-çarpımlı RGB + alfa) aynı uygulanır."""
    rgb_p, a = _onceki(rgba)
    yigin = np.concatenate([rgb_p, a * 255.0], axis=2)
    sonuc = fn(yigin)
    return _sonraki(sonuc[..., :3], sonuc[..., 3:4] / 255.0, rgba)


def _disk(r):
    r = max(1, int(round(r)))
    y, x = np.mgrid[-r:r + 1, -r:r + 1]
    k = ((x * x + y * y) <= r * r + 0.5).astype(np.float32)
    return k / k.sum()


# ------------------------------------------------------------------ Alan derinliği (Depth Anything haritasıyla)
def _disk_bulanik(im, r):
    """Disk (lens) bulanıklığı; büyük yarıçapta küçültülmüş resimde yapılıp büyütülür (hızlı)."""
    if r < 0.6:
        return im
    H, W = im.shape[:2]
    if r <= 12:
        return cv2.filter2D(im, -1, _disk(r), borderType=cv2.BORDER_REFLECT)
    s = r / 10.0
    k = cv2.resize(im, (max(1, int(W / s)), max(1, int(H / s))), interpolation=cv2.INTER_AREA)
    k = cv2.filter2D(k, -1, _disk(10), borderType=cv2.BORDER_REFLECT)
    return cv2.resize(k, (W, H), interpolation=cv2.INTER_LINEAR)


def dof(rgba, D, p):
    """D: derinlik (0 uzak – 1 yakın). Odak noktası (fx, fy: 0–1) keskin kalır; odaktan derinlikçe uzaklaştıkça
    bulanıklık artar. miktar 0–100 (en büyük yarıçap = kısa kenarın %4'ü), aralik 0–100 (net kalan derinlik bandı),
    yon: iki | uzak (yalnız arkası) | yakin (yalnız önü)."""
    H, W = rgba.shape[:2]
    D = cv2.resize(D, (W, H), interpolation=cv2.INTER_LINEAR)
    fx, fy = int(np.clip(float(p.get("fx", 0.5)) * W, 0, W - 1)), int(np.clip(float(p.get("fy", 0.5)) * H, 0, H - 1))
    y0, y1, x0, x1 = max(0, fy - 3), fy + 4, max(0, fx - 3), fx + 4
    f = float(np.median(D[y0:y1, x0:x1]))
    rmax = float(p.get("miktar", 40)) / 100.0 * 0.04 * min(W, H)
    band = float(p.get("aralik", 15)) / 100.0 * 0.5
    fark = D - f
    yon = p.get("yon", "iki")
    if yon == "uzak":
        fark = np.minimum(fark, 0)
    elif yon == "yakin":
        fark = np.maximum(fark, 0)
    t = np.clip((np.abs(fark) - band) / max(1e-3, 0.55 - band), 0, 1)
    r = rmax * t
    if rmax < 0.6:
        return rgba
    seviye = [0.0] + [rmax * (i / 6) ** 1.5 for i in range(1, 7)]
    rgb_p, a = _onceki(rgba)
    yigin = np.concatenate([rgb_p, a * 255.0], axis=2)
    katlar = [_disk_bulanik(yigin, s) for s in seviye]
    out = np.zeros_like(yigin)
    for i in range(len(seviye) - 1):
        lo, hi = seviye[i], seviye[i + 1]
        w = np.clip((r - lo) / max(1e-6, hi - lo), 0, 1)[..., None]
        sec = ((r >= lo) & ((r < hi) | (i == len(seviye) - 2)))[..., None]
        out = np.where(sec, katlar[i] * (1 - w) + katlar[i + 1] * w, out)
    return _sonraki(out[..., :3], out[..., 3:4] / 255.0, rgba)


# ------------------------------------------------------------------ Gökyüzü değiştirme (Photoshop Sky Replacement benzeri)
def gokyuzu(rgba, maske, gok, p):
    """maske: 0–255 gökyüzü maskesi (SegFormer + kılavuzlu süzgeç), gok: RGB gökyüzü resmi.
    kenar (-30..30 px, + gökyüzünü büyütür), yumusak (0..30 px), olcek (%50..300), kx/ky (-100..100),
    parlak/sicaklik (-100..100, yalnız gökyüzü), uyum (0..100: ön planın rengi/ışığı yeni gökyüzüne uyar),
    isik (0..100: kenarda gökyüzü ışığı ön plana taşar)."""
    H, W = rgba.shape[:2]
    sc = min(W, H) / 1000.0                                          # piksel ayarları 1000 px'e göre ölçeklenir
    m = cv2.resize(maske, (W, H), interpolation=cv2.INTER_LINEAR).astype(np.float32) / 255.0
    kenar = float(p.get("kenar", 0)) * sc
    if abs(kenar) >= 0.5:
        k = int(round(abs(kenar))) * 2 + 1
        el = cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (k, k))
        m = cv2.dilate(m, el) if kenar > 0 else cv2.erode(m, el)
    yum = float(p.get("yumusak", 3)) * sc
    if yum > 0.3:
        m = cv2.GaussianBlur(m, (0, 0), yum)
    m = np.clip(m, 0, 1)
    # gökyüzü yerleşimi: gökyüzü bölgesinin (en alt satırına kadar) üstünü kaplar
    satir = np.where(m.max(axis=1) > 0.5)[0]
    alt = int(satir.max()) + 1 if len(satir) else H // 2
    bh = max(int(H * 0.25), alt + int(H * 0.05))
    gh, gw = gok.shape[:2]
    olc = max(0.5, float(p.get("olcek", 100)) / 100.0)
    k = max(W / gw, bh / gh) * olc
    nw, nh = max(W, int(gw * k)), max(bh, int(gh * k))
    g = cv2.resize(gok, (nw, nh), interpolation=cv2.INTER_AREA if k < 1 else cv2.INTER_CUBIC).astype(np.float32)
    ox = int((nw - W) / 2 * (1 + float(p.get("kx", 0)) / 100.0))
    oy = int((nh - bh) / 2 * (1 + float(p.get("ky", 0)) / 100.0))
    g = g[oy:oy + bh, ox:ox + W]
    tuval = np.zeros((H, W, 3), np.float32)
    tuval[:min(bh, H)] = g[:min(bh, H)]
    if bh < H:                                                         # alt kısım (gerekirse) son satırla doldurulur
        tuval[bh:] = g[-1:]
    parlak, sicak = float(p.get("parlak", 0)) / 100.0, float(p.get("sicaklik", 0)) / 100.0
    tuval = tuval * (1 + parlak * 0.6)
    tuval[..., 0] *= 1 + sicak * 0.12
    tuval[..., 2] *= 1 - sicak * 0.12
    tuval = np.clip(tuval, 0, 255)
    rgb = rgba[..., :3].astype(np.float32)
    # ön plan uyumu: Lab'da gökyüzünün ortalama renk/ışık farkı ön plana taşınır
    uyum = float(p.get("uyum", 50)) / 100.0
    if uyum > 0:
        lab_o = cv2.cvtColor(rgb.astype(np.uint8), cv2.COLOR_RGB2LAB).astype(np.float32)
        lab_g = cv2.cvtColor(tuval.astype(np.uint8), cv2.COLOR_RGB2LAB).astype(np.float32)
        wm = m[..., None]
        if wm.sum() > 10:
            eski = (lab_o * wm).sum((0, 1)) / wm.sum()
            yeni = (lab_g * wm).sum((0, 1)) / wm.sum()
            fark = (yeni - eski) * np.array([0.25, 0.6, 0.6], np.float32) * uyum
            on = np.clip(lab_o + fark, 0, 255).astype(np.uint8)
            rgb = cv2.cvtColor(on, cv2.COLOR_LAB2RGB).astype(np.float32) * (1 - wm) + rgb * wm
    # kenar ışığı: gökyüzünün yumuşak ışığı kenara yakın ön plana sızar
    isik = float(p.get("isik", 20)) / 100.0
    if isik > 0:
        r = max(3.0, 25 * sc)
        yayi = cv2.GaussianBlur(m, (0, 0), r)
        renk = cv2.GaussianBlur(tuval * m[..., None], (0, 0), r) / np.maximum(yayi[..., None], 1e-3)
        bant = (np.clip(yayi - m, 0, 1) * isik * 1.5)[..., None]
        rgb = rgb * (1 - bant) + (rgb * 0.5 + renk * 0.5) * bant
    out = rgba.copy()
    out[..., :3] = np.clip(tuval * m[..., None] + rgb * (1 - m[..., None]) + 0.5, 0, 255).astype(np.uint8)
    return out


