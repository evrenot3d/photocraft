"""EVREN yapay zekâ motoru — PhotoCraft'ın sol menüdeki Upscale / Alan derinliği / Bitki / Cam / Gökyüzü /
İnsanları düzelt araçları.

PhotoCraft bu betiği yalnız bir araç çalışırken başlatır ve iş bitince (ya da pencere kapanınca) kapatır:
sürekli arkada çalışan bir süreç yok. ComfyUI (Upscale için) de yalnız upscale işi sırasında açılır.

Konuşma: standart girdiden her satıra bir JSON istek, standart çıktıya her satıra bir JSON cevap.
  istek : {"id": 1, "is": "upscale" | "ayir" | "derinlik" | "dof" | "gok" | "gok_liste" | "gok_ekle" | "insan", ...}
  cevap : {"id": 1, "durum": "ilerleme", "asama": "...", "yuzde": 0.4}
          {"id": 1, "durum": "tamam", ...}   ya da   {"id": 1, "durum": "hata", "mesaj": "..."}
Girdi bitince (PhotoCraft kapatınca) ComfyUI da kapatılır ve süreç biter.
"""
import json
import os
import random
import sys
import threading
import traceback

KOK = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, KOK)
os.environ.setdefault("PYTHONUTF8", "1")
for _akis in (sys.stdin, sys.stdout, sys.stderr):
    try:
        _akis.reconfigure(encoding="utf-8")
    except Exception:  # noqa: BLE001
        pass

import numpy as np  # noqa: E402
from PIL import Image, ImageFilter  # noqa: E402

GOK_KLASOR = os.path.join(KOK, "gokyuzu_kutuphanesi")

_yaz_kilit = threading.Lock()
# Mesaj dili: her istekle gelir ("dil": "tr" | "en"; PhotoCraft'ın arayüz diline göre).
DIL = "en"


def _t(en, tr):
    """İki dilli metin: _t("English", "Türkçe")."""
    return tr if DIL == "tr" else en

_motor = None
_iptal = threading.Event()


def yaz(v):
    with _yaz_kilit:
        sys.stdout.write(json.dumps(v, ensure_ascii=False) + "\n")
        sys.stdout.flush()


def ilerle(rid, asama, yuzde=None):
    yaz({"id": rid, "durum": "ilerleme", "asama": asama, "yuzde": yuzde})


# ------------------------------------------------------------------ resim önbelleği (pencere açıkken aynı belge)
_resimler = {}


def resim(yol):
    """RGBA uint8 dizi; aynı dosya ikinci kez okunmaz."""
    st = os.stat(yol)
    k = (yol, st.st_mtime_ns, st.st_size)
    if k not in _resimler:
        _resimler.clear()
        _resimler[k] = np.asarray(Image.open(yol).convert("RGBA")).copy()
    return _resimler[k]


def kucult(rgba, en_cok):
    """Önizleme için uzun kenarı en_cok piksele indirir (oran: kucuk / asil)."""
    H, W = rgba.shape[:2]
    if not en_cok or max(W, H) <= en_cok:
        return rgba, 1.0
    k = en_cok / max(W, H)
    import cv2
    return cv2.resize(rgba, (max(1, int(W * k)), max(1, int(H * k))), interpolation=cv2.INTER_AREA), k


def kaydet(rgba, yol):
    os.makedirs(os.path.dirname(yol), exist_ok=True)
    gecici = yol + ".tmp.png"
    Image.fromarray(rgba).save(gecici, compress_level=1)
    os.replace(gecici, yol)


# ------------------------------------------------------------------ Upscale (ComfyUI: SUPIR / UltraSharp)
VARSAYILAN_PROMPT = ("high quality photo, highly detailed, sharp focus, realistic textures, "
                     "natural lighting, clean edges")
VARSAYILAN_NEG = "blurry, lowres, noise, jpeg artifacts, watermark, text, deformed, distorted, oversaturated"
SUPIR_MODEL = "SUPIR-v0Q_fp16.safetensors"
SDXL_MODEL = "Juggernaut-XL_v9_RunDiffusionPhoto_v2.safetensors"
ULTRASHARP = "4x-UltraSharp.safetensors"


class WF:
    def __init__(self):
        self.g, self.n = {}, 0

    def dugum(self, sinif, **girdi):
        self.n += 1
        self.g[str(self.n)] = {"class_type": sinif, "inputs": girdi}
        return str(self.n)

    @staticmethod
    def c(nid, cikis=0):
        return [nid, cikis]


def tamamla(g, obj):
    """object_info'ya göre eksik zorunlu girdileri varsayılanla doldurur (eklenti sürümü değişse de çalışsın)."""
    for d in g.values():
        tanim = obj.get(d["class_type"])
        if not tanim:
            raise KeyError(f"Motor bu düğümü tanımıyor: {d['class_type']}")
        for ad, v in (tanim.get("input", {}).get("required") or {}).items():
            if ad in d["inputs"]:
                continue
            tip, ek = v[0], (v[1] if len(v) > 1 and isinstance(v[1], dict) else {})
            if "default" in ek:
                d["inputs"][ad] = ek["default"]
            elif isinstance(tip, list) and tip:
                d["inputs"][ad] = tip[0]
            elif tip == "COMBO" and ek.get("options"):
                d["inputs"][ad] = ek["options"][0]
    return g


def upscale_wf(girdi_adi, W, H, model, adim):
    wf = WF()
    img = wf.dugum("LoadImage", image=girdi_adi)
    if model == "hizli":
        um = wf.dugum("UpscaleModelLoader", model_name=ULTRASHARP)
        up = wf.dugum("ImageUpscaleWithModel", upscale_model=WF.c(um), image=WF.c(img))
        son = wf.dugum("ImageScale", image=WF.c(up), upscale_method="lanczos", width=W, height=H, crop="disabled")
    else:
        ck = wf.dugum("CheckpointLoaderSimple", ckpt_name=SDXL_MODEL)
        sm = wf.dugum("SUPIR_model_loader_v2", model=WF.c(ck, 0), clip=WF.c(ck, 1), vae=WF.c(ck, 2),
                      supir_model=SUPIR_MODEL, fp8_unet=True, diffusion_dtype="auto", high_vram=False)
        buyuk = wf.dugum("ImageScale", image=WF.c(img), upscale_method="lanczos", width=W, height=H, crop="disabled")
        fs = wf.dugum("SUPIR_first_stage", SUPIR_VAE=WF.c(sm, 1), image=WF.c(buyuk), use_tiled_vae=True,
                      encoder_tile_size=512, decoder_tile_size=512, encoder_dtype="auto")
        cond = wf.dugum("SUPIR_conditioner", SUPIR_model=WF.c(sm, 0), latents=WF.c(fs, 2),
                        positive_prompt=VARSAYILAN_PROMPT, negative_prompt=VARSAYILAN_NEG)
        parca = max(256, min(1024, min(W, H) // 64 * 64))
        sp = wf.dugum("SUPIR_sample", SUPIR_model=WF.c(sm, 0), latents=WF.c(fs, 2), positive=WF.c(cond, 0),
                      negative=WF.c(cond, 1), seed=random.randint(1, 2 ** 31), steps=int(adim or 30),
                      cfg_scale_start=4.0, cfg_scale_end=4.0, EDM_s_churn=5, s_noise=1.003, DPMPP_eta=1.0,
                      control_scale_start=1.0, control_scale_end=1.0, restore_cfg=-1.0, keep_model_loaded=False,
                      sampler="TiledRestoreEDMSampler", sampler_tile_size=parca,
                      sampler_tile_stride=max(128, parca // 2))
        son = wf.dugum("SUPIR_decode", SUPIR_VAE=WF.c(fs, 0), latents=WF.c(sp), use_tiled_vae=True,
                       decoder_tile_size=512, decoder_dtype="auto")
    kay = wf.dugum("SaveImage", images=WF.c(son), filename_prefix="evren_up")
    return wf.g, kay


def _asama(sinif):
    s = sinif.lower()
    if "supir_sample" in s:
        return _t("SUPIR adding detail", "SUPIR detay ekliyor")
    if "supir_first_stage" in s:
        return _t("SUPIR preparing", "SUPIR hazırlık")
    if "decode" in s:
        return _t("Decoding", "Çözülüyor")
    if "loader" in s:
        return _t("Loading model", "Model yükleniyor")
    if "imageupscalewithmodel" in s:
        return _t("Upscaling", "Büyütülüyor")
    return _t("Working", "Çalışıyor")


def is_upscale(rid, v):
    """{girdi, cikti, genislik, yukseklik, model: supir|hizli, adim}"""
    global _motor
    import motor
    if _motor is None:
        _motor = motor.Motor()
    if not _motor.calisiyor():
        ilerle(rid, _t("Starting the engine (ComfyUI)", "Motor (ComfyUI) açılıyor"), None)
        _motor.baslat()
    os.makedirs(motor.GIRDI, exist_ok=True)
    ad = f"evren_up_{rid}.png"
    Image.open(v["girdi"]).convert("RGB").save(os.path.join(motor.GIRDI, ad))
    W, H = int(v["genislik"]), int(v["yukseklik"])
    g, kay = upscale_wf(ad, W, H, v.get("model", "supir"), v.get("adim"))
    tamamla(g, _motor.object_info())

    def ilerleme(olay, veri):
        if olay == "node":
            ilerle(rid, _asama(g.get(str(veri), {}).get("class_type", "")), 0.0)
        elif olay == "progress":
            d, mx = veri
            ilerle(rid, None, min(0.99, d / max(1, mx)))

    _iptal.clear()
    try:
        cikti = _motor.calistir(g, ilerleme=ilerleme, iptal=_iptal.is_set)
    finally:
        try:
            os.remove(os.path.join(motor.GIRDI, ad))
        except OSError:
            pass
    res = (cikti.get(kay) or {}).get("images") or []
    if not res:
        raise RuntimeError(_t("The engine produced no output", "Motor çıktı üretmedi"))
    os.makedirs(os.path.dirname(v["cikti"]), exist_ok=True)
    os.replace(motor.cikti_yolu(res[0]), v["cikti"])
    return {"cikti": v["cikti"], "genislik": W, "yukseklik": H}


# ------------------------------------------------------------------ Bitkileri / camları ayır
def is_ayir(rid, v):
    """{tur: bitki|cam, girdi, cikti} → yalnız bitkiler / camlar, saydam PNG (resim boyutunda)."""
    import gorus
    tur = v["tur"]
    rgba = resim(v["girdi"])
    img = Image.fromarray(rgba).convert("RGB")
    anahtar = f"{v['girdi']}:{rgba.shape}"
    ilerle(rid, _t("Finding plants", "Bitkiler aranıyor") if tur == "bitki" else _t("Finding glass", "Camlar aranıyor"), None)
    if tur == "bitki":
        mk = np.maximum(gorus.sinif_maskesi(anahtar, img, "bitki", rafine=True),
                        gorus.metin_maskesi(anahtar, img, "potted plant. tree. bush. plant.", 0.3))
    else:
        mk = np.maximum(gorus.metin_maskesi(anahtar, img, "window. glass door. glass.", 0.3),
                        gorus.sinif_maskesi(anahtar, img, "cam", rafine=False))
    if mk.mean() < 0.3:
        raise RuntimeError((_t("No plants / trees found in the image", "Görselde bitki / ağaç bulunamadı") if tur == "bitki" else _t("No windows / glass found in the image", "Görselde pencere / cam bulunamadı")))
    ilerle(rid, _t("Separating", "Ayrılıyor"), 0.9)
    out = Image.fromarray(rgba)
    a = np.asarray(Image.fromarray(mk).filter(ImageFilter.GaussianBlur(0.7)), np.float32) / 255.0
    arr = np.asarray(out).copy()
    arr[..., 3] = (arr[..., 3].astype(np.float32) * a).astype(np.uint8)
    kaydet(arr, v["cikti"])
    return {"cikti": v["cikti"], "kapsam": float(mk.mean() / 255.0)}


# ------------------------------------------------------------------ Alan derinliği
_derinlik = {}


def derinlik(yol):
    import gorus
    rgba = resim(yol)
    k = (yol, rgba.shape)
    if k not in _derinlik:
        _derinlik.clear()
        _derinlik[k] = gorus.derinlik(f"{yol}:{rgba.shape}", Image.fromarray(rgba).convert("RGB"))
    return _derinlik[k]


def is_derinlik(rid, v):
    ilerle(rid, _t("Computing depth", "Derinlik hesaplanıyor"), None)
    derinlik(v["girdi"])
    return {}


def is_dof(rid, v):
    """{girdi, cikti, ayar: {fx, fy, miktar, aralik, yon}, onizleme: en uzun kenar | null}"""
    import cv2
    import efekt
    rgba = resim(v["girdi"])
    D = derinlik(v["girdi"])
    kucuk, k = kucult(rgba, v.get("onizleme"))
    if k != 1.0:
        D = cv2.resize(D, (kucuk.shape[1], kucuk.shape[0]), interpolation=cv2.INTER_AREA)
    if not v.get("onizleme"):
        ilerle(rid, _t("Blurring", "Bulanıklaştırılıyor"), None)
    out = efekt.dof(kucuk, D, v.get("ayar") or {})
    kaydet(out, v["cikti"])
    return {"cikti": v["cikti"], "genislik": out.shape[1], "yukseklik": out.shape[0]}


# ------------------------------------------------------------------ Gökyüzü değiştirme
_gok_maske = {}


def gok_maskesi(yol):
    import gorus
    rgba = resim(yol)
    k = (yol, rgba.shape)
    if k not in _gok_maske:
        _gok_maske.clear()
        _gok_maske[k] = gorus.sinif_maskesi(f"{yol}:{rgba.shape}", Image.fromarray(rgba).convert("RGB"), "gokyuzu", rafine=True)
    return _gok_maske[k]


def _kucuk_resim(ad):
    """Kütüphane küçük resmi (_k klasörü, 320 px)."""
    kaynak = os.path.join(GOK_KLASOR, ad)
    hedef = os.path.join(GOK_KLASOR, "_k", os.path.splitext(ad)[0] + ".png")
    if not os.path.isfile(hedef) or os.path.getmtime(hedef) < os.path.getmtime(kaynak):
        os.makedirs(os.path.dirname(hedef), exist_ok=True)
        im = Image.open(kaynak).convert("RGB")
        im.thumbnail((320, 320))
        im.save(hedef)
    return hedef


def is_gok_liste(rid, v):
    os.makedirs(GOK_KLASOR, exist_ok=True)
    adlar = sorted(f for f in os.listdir(GOK_KLASOR)
                   if os.path.isfile(os.path.join(GOK_KLASOR, f)) and f.lower().endswith((".jpg", ".jpeg", ".png", ".webp", ".tif", ".tiff")))
    return {"liste": [{"ad": a, "yol": os.path.join(GOK_KLASOR, a), "kucuk": _kucuk_resim(a)} for a in adlar]}


def is_gok_ekle(rid, v):
    import shutil
    os.makedirs(GOK_KLASOR, exist_ok=True)
    ad = os.path.basename(v["yol"])
    hedef = os.path.join(GOK_KLASOR, ad)
    if os.path.abspath(hedef) != os.path.abspath(v["yol"]):
        shutil.copy2(v["yol"], hedef)
    return {"ad": ad}


_gok_resim = {}


def gok_resmi(yol):
    if yol not in _gok_resim:
        _gok_resim.clear()
        _gok_resim[yol] = np.asarray(Image.open(yol).convert("RGB"))
    return _gok_resim[yol]


def is_gok(rid, v):
    """{girdi, gok (kütüphane resmi), cikti, ayar, onizleme}"""
    import cv2
    import efekt
    rgba = resim(v["girdi"])
    if not _gok_maske:
        ilerle(rid, _t("Finding the sky", "Gökyüzü aranıyor"), None)
    m = gok_maskesi(v["girdi"])
    if m.mean() < 0.5:
        raise RuntimeError(_t("No sky found in the image", "Görselde gökyüzü bulunamadı"))
    kucuk, k = kucult(rgba, v.get("onizleme"))
    if k != 1.0:
        m = cv2.resize(m, (kucuk.shape[1], kucuk.shape[0]), interpolation=cv2.INTER_AREA)
    out = efekt.gokyuzu(kucuk, m, gok_resmi(v["gok"]), v.get("ayar") or {})
    kaydet(out, v["cikti"])
    return {"cikti": v["cikti"], "genislik": out.shape[1], "yukseklik": out.shape[0]}


# ------------------------------------------------------------------ İnsanları düzelt (Detailer)
# ComfyUI Impact Pack'teki FaceDetailer / ADetailer yöntemi, kurulu modellerle ve EN GÜÇLÜ ayarda ("tamamen yeniden
# çiz"): Grounding DINO insanları bulur (uzaktaki küçük insanlar için resme parça parça bakılır), her insanın çevresi
# kesilip büyütülür (4x-UltraSharp), SAM 2.1 sınırını, Depth Anything derinliğini çıkarır. Juggernaut XL insanı
# derinlik ControlNet'i (xinsir Union promax) duruşu ve yeri korurken baştan çizer; görünen yüz ayrıca 1024 px'e
# büyütülüp sıfırdan çizilir. Sonuç küçültülüp yumuşak kenarla yerine konur. Çıktı: yalnız düzeltilen insanlar,
# saydam PNG (resim boyutunda). ControlNet dosyası yoksa ControlNet'siz çalışır.
INSAN_POZ = ("realistic photo of a person, natural human face, detailed natural face, natural body proportions, "
             "correct anatomy, realistic clothing, sharp focus, architectural photography, natural light")
INSAN_NEG = ("deformed face, distorted face, asymmetric eyes, melted face, bad anatomy, extra limbs, extra arms, "
             "extra legs, missing limbs, mutated, disfigured, blurry, lowres, cartoon, anime, painting, illustration, "
             "cgi, plastic skin, text, watermark")
INSAN_HEDEF = 1024           # kesilen parçanın büyütüldüğü uzun kenar (SDXL'in rahat çalıştığı boyut)
INSAN_EN_BUYUK = 640         # bundan uzun insan kutuları atlanır (yakındaki insanlar zaten bozulmaz)
INSAN_EN_KUCUK = 10          # bundan kısa kutular atlanır (tanınamayacak kadar küçük)
INSAN_EN_COK = 60
INSAN_ADIM = 30
GOVDE_GUC = 0.75             # vücut: ne kadar yeniden çizilsin (duruşu ControlNet korur)
YUZ_GUC = 0.72               # yüz: eski bozuk yüzün şekli neredeyse hiç korunmaz
CN_DOSYA = "controlnet-union-sdxl-1.0-promax.safetensors"
YUZ_POZ = ("close-up photo of a human face, natural realistic face, symmetrical eyes, natural eyes and nose and mouth, "
           "detailed skin texture, natural expression, sharp focus, natural light, photorealistic")
YUZ_NEG = ("deformed face, distorted face, asymmetric face, asymmetric eyes, cross-eyed, melted face, extra eyes, "
           "bad teeth, blurry, lowres, cartoon, anime, painting, illustration, cgi, plastic skin, doll, mask, "
           "text, watermark")


def _ortusme(a, b):
    """Kesişimin küçük kutuya oranı."""
    x0, y0, x1, y1 = max(a[0], b[0]), max(a[1], b[1]), min(a[2], b[2]), min(a[3], b[3])
    if x1 <= x0 or y1 <= y0:
        return 0.0
    ka = (a[2] - a[0]) * (a[3] - a[1])
    kb = (b[2] - b[0]) * (b[3] - b[1])
    return (x1 - x0) * (y1 - y0) / max(1e-6, min(ka, kb))


def insan_bul(rgb, rid):
    """Bölgedeki insan kutuları [(x0, y0, x1, y1, skor)]. Büyük bölgede ~640 piksellik örtüşen parçalara da bakılır:
    Grounding DINO resmi ~800 piksele indirdiği için tek bakışta uzaktaki küçük insanları kaçırır."""
    import gorus
    H, W = rgb.shape[:2]
    img = Image.fromarray(rgb)
    bulunan = [(b, s) for b, s in gorus.kutu_bul(img, "person.", 0.28)]
    T = int(max(640, (W * H / 40) ** 0.5))
    if max(W, H) > T * 1.3:
        adim = int(T * 0.75)
        xs = sorted(set(list(range(0, max(1, W - T), adim)) + [max(0, W - T)]))
        ys = sorted(set(list(range(0, max(1, H - T), adim)) + [max(0, H - T)]))
        n, i = len(xs) * len(ys), 0
        for y in ys:
            for x in xs:
                i += 1
                ilerle(rid, _t(f"Finding people ({i}/{n})", f"İnsanlar aranıyor ({i}/{n})"), 0.15 * i / n)
                parca = img.crop((x, y, min(W, x + T), min(H, y + T)))
                for b, s in gorus.kutu_bul(parca, "person.", 0.28):
                    bulunan.append(([b[0] + x, b[1] + y, b[2] + x, b[3] + y], s))
    # Büyükten küçüğe; büyük ölçüde başka bir kutunun içinde kalan (parça kenarında yarım kalmış) kutular atılır.
    bulunan.sort(key=lambda t: -(t[0][2] - t[0][0]) * (t[0][3] - t[0][1]))
    secilen = []
    for b, s in bulunan:
        h = b[3] - b[1]
        if h < INSAN_EN_KUCUK or h > 0.9 * H:
            continue
        if any(_ortusme(b, c) > 0.6 for c, _ in secilen):
            continue
        secilen.append((b, s))
    secilen.sort(key=lambda t: -t[1])
    return [(*b, s) for b, s in secilen[:INSAN_EN_COK]]


def _depth_png(D):
    """Derinlik (0 uzak – 1 yakın) → ControlNet'in beklediği gri resim (yakın beyaz)."""
    return Image.fromarray((np.clip(D, 0, 1) * 255).astype(np.uint8)).convert("RGB")


def _elips_maske(w, h, kutu, buyut=1.0):
    import cv2
    m = np.zeros((h, w), np.uint8)
    cx, cy = (kutu[0] + kutu[2]) / 2, (kutu[1] + kutu[3]) / 2
    rx, ry = (kutu[2] - kutu[0]) / 2 * buyut, (kutu[3] - kutu[1]) / 2 * buyut
    cv2.ellipse(m, (int(cx), int(cy)), (max(1, int(rx)), max(1, int(ry))), 0, 0, 360, 255, -1)
    return m


def insan_wf(girdi_adi, maske_adi, derinlik_adi, tw, th, poz, neg, denoise, cn_guc, cn_son, buyut):
    """Maskeli yeniden çizim. buyut: girdi küçük parça (4x-UltraSharp + Lanczos ile tw×th'ye); değilse zaten tw×th.
    derinlik_adi: duruşu / şekli koruyan derinlik ControlNet'i (Union promax, "depth"); None ise ControlNet yok."""
    wf = WF()
    ck = wf.dugum("CheckpointLoaderSimple", ckpt_name=SDXL_MODEL)
    p = wf.dugum("CLIPTextEncode", text=poz, clip=WF.c(ck, 1))
    n = wf.dugum("CLIPTextEncode", text=neg, clip=WF.c(ck, 1))
    img = wf.dugum("LoadImage", image=girdi_adi)
    if buyut:
        um = wf.dugum("UpscaleModelLoader", model_name=ULTRASHARP)
        img = wf.dugum("ImageUpscaleWithModel", upscale_model=WF.c(um), image=WF.c(img))
    buyuk = wf.dugum("ImageScale", image=WF.c(img), upscale_method="lanczos", width=tw, height=th, crop="disabled")
    lat = wf.dugum("VAEEncode", pixels=WF.c(buyuk), vae=WF.c(ck, 2))
    mk = wf.dugum("LoadImageMask", image=maske_adi, channel="red")
    ml = wf.dugum("SetLatentNoiseMask", samples=WF.c(lat), mask=WF.c(mk))
    pc, nc = WF.c(p), WF.c(n)
    if derinlik_adi:
        cn = wf.dugum("ControlNetLoader", control_net_name=CN_DOSYA)
        cnt = wf.dugum("SetUnionControlNetType", control_net=WF.c(cn), type="depth")
        dimg = wf.dugum("LoadImage", image=derinlik_adi)
        dimg = wf.dugum("ImageScale", image=WF.c(dimg), upscale_method="bilinear", width=tw, height=th, crop="disabled")
        ap = wf.dugum("ControlNetApplyAdvanced", positive=pc, negative=nc, control_net=WF.c(cnt), image=WF.c(dimg),
                      strength=float(cn_guc), start_percent=0.0, end_percent=float(cn_son), vae=WF.c(ck, 2))
        pc, nc = WF.c(ap, 0), WF.c(ap, 1)
    ks = wf.dugum("KSampler", model=WF.c(ck, 0), positive=pc, negative=nc, latent_image=WF.c(ml),
                  seed=random.randint(1, 2 ** 31), steps=INSAN_ADIM, cfg=5.0, sampler_name="dpmpp_2m",
                  scheduler="karras", denoise=float(denoise))
    dec = wf.dugum("VAEDecode", samples=WF.c(ks), vae=WF.c(ck, 2))
    kay = wf.dugum("SaveImage", images=WF.c(dec), filename_prefix="evren_insan")
    return wf.g, kay


def _comfy_ciz(rid, obj, adlar, resimler, wf_arg, ilerleme):
    """resimler: {ad: PIL} motorun girdi klasörüne yazılır; iş çalışır; sonuç RGB dizi (ya da None)."""
    import motor
    for ad, im in resimler.items():
        im.save(os.path.join(motor.GIRDI, ad))
    g, kay = insan_wf(*wf_arg)
    tamamla(g, obj)
    try:
        cikti = _motor.calistir(g, ilerleme=ilerleme)
    finally:
        for ad in resimler:
            try:
                os.remove(os.path.join(motor.GIRDI, ad))
            except OSError:
                pass
    res = (cikti.get(kay) or {}).get("images") or []
    if not res:
        return None
    yol = motor.cikti_yolu(res[0])
    sonuc = np.asarray(Image.open(yol).convert("RGB"))
    try:
        os.remove(yol)
    except OSError:
        pass
    return sonuc


def is_insan(rid, v):
    """{girdi (görünen hâl PNG), cikti, bolge: [x, y, w, h] | null (seçim; yoksa bütün resim)}
    → yalnız düzeltilen insanlar, saydam PNG (resim boyutunda). İki aşama, en güçlü ayar ("tamamen yeniden çiz"):
    1) vücut: derinlik ControlNet'i duruşu ve yeri korurken insan baştan çizilir;
    2) yüz (görünüyorsa): baş ayrıca 1024 px'e büyütülüp sıfırdan çizilir, vücudun üstüne konur."""
    global _motor
    import cv2
    import gorus
    import motor
    rgba = resim(v["girdi"])
    H, W = rgba.shape[:2]
    rgb = np.ascontiguousarray(rgba[..., :3])
    bx, by, bw, bh = v.get("bolge") or [0, 0, W, H]
    bx, by = max(0, int(bx)), max(0, int(by))
    bw, bh = min(W - bx, int(bw)), min(H - by, int(bh))
    if bw < 8 or bh < 8:
        raise RuntimeError(_t("The selected area is too small", "Seçili alan çok küçük"))
    cn_var = os.path.isfile(os.path.join(motor.MODELLER, "controlnet", CN_DOSYA))
    ilerle(rid, _t("Finding people", "İnsanlar aranıyor"), 0.0)
    kisiler = insan_bul(rgb[by:by + bh, bx:bx + bw], rid)
    kisiler = [(x0 + bx, y0 + by, x1 + bx, y1 + by, s) for x0, y0, x1, y1, s in kisiler]
    atlanan = sum(1 for k in kisiler if k[3] - k[1] > INSAN_EN_BUYUK)
    kisiler = [k for k in kisiler if k[3] - k[1] <= INSAN_EN_BUYUK]
    yer = _t("in the selection", "seçili alanda") if v.get("bolge") else _t("in the image", "görselde")
    if not kisiler:
        raise RuntimeError(_t(f"No distant people to fix ({yer})", f"Düzeltilecek uzak insan bulunamadı ({yer})") if atlanan else _t(f"No people found ({yer})", f"İnsan bulunamadı ({yer})"))

    # 1) Her insan için: parça, büyütülmüş boyut, SAM maskesi, derinlik, yüz kutusu. ComfyUI'den ÖNCE yapılır ve
    #    algılama modelleri sonra bellekten atılır: ekran kartı belleği SDXL + ControlNet'e kalsın.
    isler = []
    for i, (x0, y0, x1, y1, _) in enumerate(kisiler):
        ilerle(rid, _t(f"Examining people ({i + 1}/{len(kisiler)})", f"İnsanlar inceleniyor ({i + 1}/{len(kisiler)})"), 0.15 + 0.1 * i / len(kisiler))
        kw, kh = x1 - x0, y1 - y0
        px, py = max(kw * 0.5, kh * 0.2), kh * 0.25
        cx0, cy0, cx1, cy1 = x0 - px, y0 - py, x1 + px, y1 + py
        if (cx1 - cx0) < (cy1 - cy0) * 0.5:      # çok dar parça: SDXL için en az 1:2
            ek = ((cy1 - cy0) * 0.5 - (cx1 - cx0)) / 2
            cx0, cx1 = cx0 - ek, cx1 + ek
        cx0, cy0 = max(0, int(cx0)), max(0, int(cy0))
        cx1, cy1 = min(W, int(np.ceil(cx1))), min(H, int(np.ceil(cy1)))
        cw, ch = cx1 - cx0, cy1 - cy0
        k = INSAN_HEDEF / max(cw, ch)
        tw, th = max(512, int(round(cw * k / 64)) * 64), max(512, int(round(ch * k / 64)) * 64)
        parca = Image.fromarray(rgb[cy0:cy1, cx0:cx1])
        buyuk = parca.resize((tw, th), Image.LANCZOS)
        sx, sy = tw / cw, th / ch
        kutu = [(x0 - cx0) * sx, (y0 - cy0) * sy, (x1 - cx0) * sx, (y1 - cy0) * sy]
        try:
            mk = gorus.kutu_maskesi(buyuk, [kutu])[0]
        except Exception:  # noqa: BLE001
            mk = np.zeros((th, tw), np.uint8)
        if mk.mean() < 0.5:                        # SAM bulamadıysa kutunun içinde elips
            mk = _elips_maske(tw, th, kutu)
        r = max(3, int(th * 0.02))
        mk = cv2.dilate(mk, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (2 * r + 1, 2 * r + 1)))
        D = gorus.derinlik(f"insan:{rid}:{i}", buyuk) if cn_var else None
        # Görünen yüz (arkası dönük insanda yüz çizilmez). Kişinin üst yarısındaki en güvenli "face" kutusu.
        yuz = None
        try:
            ust = [(b, s) for b, s in gorus.kutu_bul(buyuk, "face.", 0.35)
                   if b[1] < kutu[1] + (kutu[3] - kutu[1]) * 0.45 and (b[3] - b[1]) < (kutu[3] - kutu[1]) * 0.4]
            if ust:
                yuz = max(ust, key=lambda t: t[1])[0]
        except Exception:  # noqa: BLE001
            yuz = None
        isler.append({"parca": parca, "maske": mk, "kutu": (cx0, cy0, cx1, cy1), "boyut": (tw, th), "derinlik": D,
                      "yuz": yuz})
    gorus.bosalt()

    # 2) ComfyUI: önce vücut, sonra yüz.
    if _motor is None:
        _motor = motor.Motor()
    if not _motor.calisiyor():
        ilerle(rid, _t("Starting the engine (ComfyUI)", "Motor (ComfyUI) açılıyor"), 0.25)
        _motor.baslat()
    os.makedirs(motor.GIRDI, exist_ok=True)
    obj = _motor.object_info()
    if cn_var and "SetUnionControlNetType" not in obj:
        cn_var = False
    out = np.zeros((H, W, 4), np.float32)
    bolge_maske = np.zeros((H, W), np.float32)
    bolge_maske[by:by + bh, bx:bx + bw] = 1.0
    n = len(isler)
    for i, it in enumerate(isler):
        tw, th = it["boyut"]
        cx0, cy0, cx1, cy1 = it["kutu"]
        cw, ch = cx1 - cx0, cy1 - cy0
        adim = 2 if it["yuz"] is not None else 1

        def ilerleme(olay, veri, i=i, asama=0):
            if olay == "progress":
                d, mx = veri
                ilerle(rid, None, 0.3 + 0.68 * (i + (asama + min(1.0, d / max(1, mx))) / adim) / n)

        ilerle(rid, _t(f"Redrawing people ({i + 1}/{n})", f"İnsanlar yeniden çiziliyor ({i + 1}/{n})"), 0.3 + 0.68 * i / n)
        on = f"evren_insan_{rid}_{i}"
        resimler = {on + ".png": it["parca"], on + "_m.png": Image.fromarray(it["maske"]).convert("RGB")}
        d_ad = None
        if cn_var and it["derinlik"] is not None:
            d_ad = on + "_d.png"
            resimler[d_ad] = _depth_png(it["derinlik"])
        govde = _comfy_ciz(rid, obj, None, resimler,
                           (on + ".png", on + "_m.png", d_ad, tw, th, INSAN_POZ, INSAN_NEG, GOVDE_GUC, 0.85, 0.9, True),
                           ilerleme)
        if govde is None:
            continue
        # Yüz: baş çevresi kare kesilip 1024 px'e büyütülür, yalnız yüz elipsi sıfırdan çizilir.
        if it["yuz"] is not None:
            ilerle(rid, _t(f"Redrawing faces ({i + 1}/{n})", f"Yüzler yeniden çiziliyor ({i + 1}/{n})"), None)
            fx0, fy0, fx1, fy1 = it["yuz"]
            kenar = max(fx1 - fx0, fy1 - fy0) * 2.4
            mx, my = (fx0 + fx1) / 2, (fy0 + fy1) / 2
            ax0, ay0 = int(max(0, min(tw - kenar, mx - kenar / 2))), int(max(0, min(th - kenar, my - kenar / 2)))
            kenar = int(min(kenar, tw - ax0, th - ay0))
            if kenar >= 16:
                Y = 1024
                bas = Image.fromarray(govde).crop((ax0, ay0, ax0 + kenar, ay0 + kenar)).resize((Y, Y), Image.LANCZOS)
                s = Y / kenar
                yk = [(fx0 - ax0) * s, (fy0 - ay0) * s, (fx1 - ax0) * s, (fy1 - ay0) * s]
                ym = _elips_maske(Y, Y, yk, 1.3)
                yres = {on + "_y.png": bas, on + "_ym.png": Image.fromarray(ym).convert("RGB")}
                yd_ad = None
                if d_ad:
                    Dk = cv2.resize(it["derinlik"], (tw, th))[ay0:ay0 + kenar, ax0:ax0 + kenar]
                    yd_ad = on + "_yd.png"
                    yres[yd_ad] = _depth_png(cv2.resize(Dk, (Y, Y)))
                yuz_sonuc = _comfy_ciz(rid, obj, None, yres,
                                       (on + "_y.png", on + "_ym.png", yd_ad, Y, Y, YUZ_POZ, YUZ_NEG, YUZ_GUC, 0.5, 0.6, False),
                                       lambda olay, veri, i=i: ilerleme(olay, veri, i, 1))
                if yuz_sonuc is not None:
                    yk_kucuk = cv2.resize(yuz_sonuc, (kenar, kenar), interpolation=cv2.INTER_AREA).astype(np.float32)
                    a = cv2.resize(ym, (kenar, kenar), interpolation=cv2.INTER_AREA).astype(np.float32) / 255.0
                    a = cv2.GaussianBlur(a, (0, 0), max(1.0, kenar * 0.03))[..., None]
                    g = govde.astype(np.float32)
                    g[ay0:ay0 + kenar, ax0:ax0 + kenar] = yk_kucuk * a + g[ay0:ay0 + kenar, ax0:ax0 + kenar] * (1 - a)
                    govde = np.clip(g + 0.5, 0, 255).astype(np.uint8)
        yeni = np.asarray(Image.fromarray(govde).resize((cw, ch), Image.LANCZOS), np.float32)
        # Yumuşak kenarlı maske (parça boyutunda), seçimin dışı kesilir.
        a = cv2.resize(it["maske"], (cw, ch), interpolation=cv2.INTER_AREA).astype(np.float32) / 255.0
        a = cv2.GaussianBlur(a, (0, 0), max(0.6, ch * 0.006))
        a *= bolge_maske[cy0:cy1, cx0:cx1]
        a = a[..., None]
        eski = out[cy0:cy1, cx0:cx1]
        ea = eski[..., 3:4]
        na = a + ea * (1 - a)
        renk = (yeni * a + eski[..., :3] * ea * (1 - a)) / np.maximum(na, 1e-6)
        out[cy0:cy1, cx0:cx1, :3] = renk
        out[cy0:cy1, cx0:cx1, 3:4] = na
    out[..., 3] *= 255.0
    sonuc = np.clip(out + 0.5, 0, 255).astype(np.uint8)
    if sonuc[..., 3].max() == 0:
        raise RuntimeError(_t("The engine produced no result", "Motor sonuç üretmedi"))
    ilerle(rid, _t("Preparing the layer", "Katman hazırlanıyor"), 0.99)
    kaydet(sonuc, v["cikti"])
    return {"cikti": v["cikti"], "kisi": n, "atlanan": atlanan, "yuz": sum(1 for it in isler if it["yuz"] is not None),
            "controlnet": cn_var}


ISLER = {"upscale": is_upscale, "ayir": is_ayir, "derinlik": is_derinlik, "dof": is_dof, "gok": is_gok,
         "gok_liste": is_gok_liste, "gok_ekle": is_gok_ekle, "insan": is_insan}


def main():
    yaz({"durum": "hazir"})
    try:
        for satir in sys.stdin:
            satir = satir.strip()
            if not satir:
                continue
            try:
                v = json.loads(satir)
            except ValueError:
                continue
            rid = v.get("id")
            global DIL
            DIL = "tr" if v.get("dil") == "tr" else "en"
            import motor as _m
            _m.DIL = DIL
            if v.get("is") == "kapat":
                break
            fn = ISLER.get(v.get("is"))
            if not fn:
                yaz({"id": rid, "durum": "hata", "mesaj": f"unknown job: {v.get('is')}"})
                continue
            try:
                sonuc = fn(rid, v) or {}
                yaz({"id": rid, "durum": "tamam", **sonuc})
            except Exception as e:  # noqa: BLE001
                traceback.print_exc(file=sys.stderr)
                yaz({"id": rid, "durum": "hata", "mesaj": str(e) or type(e).__name__})
    finally:
        if _motor is not None:
            _motor.durdur()


if __name__ == "__main__":
    main()
