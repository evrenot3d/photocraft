"""Görüş (algılama) modelleri: tek tıkla nesne seçme, anlamsal bölütleme, derinlik.

SAM 2.1 base+ (Meta, Apache-2.0)         · tıklanan nesnenin maskesi (Shift ekle / Alt çıkar)
SegFormer-B5 ADE20K (NVIDIA)              · gökyüzü / ağaç-bitki / pencere-cam maskeleri (150 sınıf)
Depth Anything V2 Large (CC-BY-NC-4.0)    · göreli derinlik (alan derinliği bulanıklığı için)

Hepsi transformers ile sunucu sürecinde, ekran kartında çalışır; dosyalar models\\hf\\<ad> klasörüne programın kendi
indiricisiyle iner (ilk kullanımda). Sonuçlar resim anahtarına göre önbelleklenir (aynı belgeye ikinci tık anında).
10 dakika kullanılmayan model bellekten atılır.
"""
import os
import threading
import time

import numpy as np
from PIL import Image

from motor import MODELLER

HF = os.path.join(MODELLER, "hf")


def _dosyalar(repo, ad, liste):
    return [{"repo": repo, "dosya": d, "klasor": f"hf/{ad}", "boyut": b} for d, b in liste]


MODELLER = {
    "sam": {"ad": "SAM 2.1 base+ (nesne seçme)", "klasor": "sam2.1-base-plus",
            "dosyalar": _dosyalar("facebook/sam2.1-hiera-base-plus", "sam2.1-base-plus", [
                ("config.json", 5705), ("model.safetensors", 323476296), ("preprocessor_config.json", 683),
                ("processor_config.json", 95)])},
    "segformer": {"ad": "SegFormer-B5 ADE20K (gökyüzü, bitki, cam)", "klasor": "segformer-b5-ade",
                  "dosyalar": _dosyalar("nvidia/segformer-b5-finetuned-ade-640-640", "segformer-b5-ade", [
                      ("config.json", 6886), ("preprocessor_config.json", 271), ("pytorch_model.bin", 339299397)])},
    "dino": {"ad": "Grounding DINO base (metinle nesne bulma: pencere, cam)", "klasor": "grounding-dino-base",
             "dosyalar": _dosyalar("IDEA-Research/grounding-dino-base", "grounding-dino-base", [
                 ("config.json", 1737), ("model.safetensors", 933400872), ("preprocessor_config.json", 457),
                 ("special_tokens_map.json", 125), ("tokenizer.json", 711396), ("tokenizer_config.json", 1237),
                 ("vocab.txt", 231508)])},
    "derinlik": {"ad": "Depth Anything V2 Large (derinlik)", "klasor": "depth-anything-v2-large",
                 "dosyalar": _dosyalar("depth-anything/Depth-Anything-V2-Large-hf", "depth-anything-v2-large", [
                     ("config.json", 1431), ("model.safetensors", 1341322868), ("preprocessor_config.json", 775)])},
}
# ADE20K sınıf numaraları (SegFormer çıktısı)
ADE = {"gokyuzu": [2], "bitki": [4, 9, 17, 66, 72], "cam": [8]}     # sky · tree, grass, plant, flower, palm · windowpane

_kilit = threading.Lock()
_yuklu = {}                 # ad -> (model, işlemci, son kullanım)
_onbellek = {}              # (ad, anahtar) -> sonuç


def _cihaz():
    import torch
    return "cuda" if torch.cuda.is_available() else "cpu"


def _yukle(ad):
    import torch
    if ad in _yuklu:
        m, p, _ = _yuklu[ad]
        _yuklu[ad] = (m, p, time.time())
        return m, p
    yol = os.path.join(HF, MODELLER[ad]["klasor"])
    if ad == "sam":
        from transformers import Sam2Model, Sam2Processor
        p, m = Sam2Processor.from_pretrained(yol), Sam2Model.from_pretrained(yol)
    elif ad == "dino":
        from transformers import AutoModelForZeroShotObjectDetection, AutoProcessor
        p, m = AutoProcessor.from_pretrained(yol), AutoModelForZeroShotObjectDetection.from_pretrained(yol)
    elif ad == "segformer":
        from transformers import SegformerForSemanticSegmentation, SegformerImageProcessor
        p, m = SegformerImageProcessor.from_pretrained(yol), SegformerForSemanticSegmentation.from_pretrained(yol)
    else:
        from transformers import AutoImageProcessor, AutoModelForDepthEstimation
        p, m = AutoImageProcessor.from_pretrained(yol), AutoModelForDepthEstimation.from_pretrained(yol, torch_dtype=torch.float16)
    m = m.to(_cihaz()).eval()
    _yuklu[ad] = (m, p, time.time())
    return m, p


def _bosalt_dongusu():
    while True:
        time.sleep(60)
        with _kilit:
            for ad in [a for a, (_, _, t) in _yuklu.items() if time.time() - t > 600]:
                _yuklu.pop(ad, None)
                try:
                    import torch
                    torch.cuda.empty_cache()
                except Exception:
                    pass




def bosalt():
    """Ağır yapay zekâ işinden (ComfyUI) önce çağrılır: algılama modelleri ve ekran kartındaki SAM gömmeleri bırakılır,
    VRAM boşalır (8 GB'ta düzenleme modelleri tam yer bulsun). Derinlik / sınıf haritaları (işlemci belleği) kalır."""
    with _kilit:
        _yuklu.clear()
        for k in [k for k in _onbellek if k[0] == "sam"]:
            _onbellek.pop(k, None)
        try:
            import torch
            if torch.cuda.is_available():
                torch.cuda.empty_cache()
        except Exception:
            pass


def _sakla(ad, anahtar, deger):
    for k in [k for k in _onbellek if k[0] == ad][:-3]:      # her model için son birkaç belge
        _onbellek.pop(k, None)
    _onbellek[(ad, anahtar)] = deger


# ------------------------------------------------------------------ SAM 2.1: tıklamadan maske
def sam_maske(anahtar, img, noktalar):
    """noktalar: [(x, y, 1=ekle / 0=çıkar)] (resim pikseli). img yalnız ilk tıkta gerekir (gömme önbelleklenir).
    Dönüş: uint8 maske (0/255), resim boyutunda."""
    import torch
    with _kilit:
        m, p = _yukle("sam")
        dev = _cihaz()
        g = _onbellek.get(("sam", anahtar))
        if g is None:
            if img is None:
                raise KeyError("yeniden gönder")
            girdi = p(images=img.convert("RGB"), return_tensors="pt").to(dev)
            with torch.no_grad():
                emb = m.get_image_embeddings(girdi["pixel_values"])
            g = {"emb": emb, "boyut": (img.height, img.width), "girdi": girdi}
            _sakla("sam", anahtar, g)
        H, W = g["boyut"]
        nok = [[[[float(x), float(y)] for x, y, _ in noktalar]]]
        etk = [[[int(l) for _, _, l in noktalar]]]
        isl = p(images=Image.new("RGB", (W, H)), input_points=nok, input_labels=etk, return_tensors="pt").to(dev)
        with torch.no_grad():
            out = m(input_points=isl["input_points"], input_labels=isl["input_labels"], image_embeddings=g["emb"],
                    multimask_output=False)
        maske = p.post_process_masks(out.pred_masks.cpu(), isl["original_sizes"].cpu())[0]
        maske = maske.reshape(-1, H, W)[0].numpy()
        return (maske > 0).astype(np.uint8) * 255


# ------------------------------------------------------------------ Grounding DINO + SAM: metinle nesne maskesi
def metin_maskesi(anahtar, img, metin, esik=0.3, en_buyuk=0.35):
    """Metinle tarif edilen nesneleri bulur (Grounding DINO kutuları), her kutuyu SAM ile tam sınırına keser;
    maskelerin birleşimini döndürür (0/255). Örn. "window. glass door." → bütün camlar."""
    import torch
    with _kilit:
        m, p = _yukle("dino")
        dev = _cihaz()
        girdi = p(images=img.convert("RGB"), text=metin, return_tensors="pt").to(dev)
        with torch.no_grad():
            out = m(**girdi)
        sonuc = p.post_process_grounded_object_detection(out, girdi.input_ids, threshold=esik, text_threshold=esik,
                                                         target_sizes=[img.size[::-1]])[0]
        kutular = [[float(v) for v in b] for b in sonuc["boxes"].cpu()]
    W, H = img.size
    toplam = np.zeros((H, W), np.uint8)
    # çok büyük kutular (resmin çoğu) genelde bütün cephe: atlanır
    kutular = [b for b in kutular if (b[2] - b[0]) * (b[3] - b[1]) < en_buyuk * W * H]
    if not kutular:
        return toplam
    with _kilit:
        sm, sp = _yukle("sam")
        g = _onbellek.get(("sam", anahtar))
        if g is None:
            gi = sp(images=img.convert("RGB"), return_tensors="pt").to(dev)
            with torch.no_grad():
                g = {"emb": sm.get_image_embeddings(gi["pixel_values"]), "boyut": (H, W)}
            _sakla("sam", anahtar, g)
        for i in range(0, len(kutular), 16):
            isl = sp(images=Image.new("RGB", (W, H)), input_boxes=[kutular[i:i + 16]], return_tensors="pt").to(dev)
            with torch.no_grad():
                o = sm(input_boxes=isl["input_boxes"], image_embeddings=g["emb"], multimask_output=False)
            mm = sp.post_process_masks(o.pred_masks.cpu(), isl["original_sizes"].cpu())[0].reshape(-1, H, W).numpy()
            toplam |= (mm > 0).any(0).astype(np.uint8) * 255
    return toplam


def kutu_bul(img, metin, esik=0.3):
    """Grounding DINO: metinle tarif edilen nesnelerin kutuları ve güvenleri [([x0, y0, x1, y1], skor)] (resim pikseli)."""
    import torch
    with _kilit:
        m, p = _yukle("dino")
        girdi = p(images=img.convert("RGB"), text=metin, return_tensors="pt").to(_cihaz())
        with torch.no_grad():
            out = m(**girdi)
        s = p.post_process_grounded_object_detection(out, girdi.input_ids, threshold=esik, text_threshold=esik,
                                                     target_sizes=[img.size[::-1]])[0]
        return [([float(v) for v in b], float(sc)) for b, sc in zip(s["boxes"].cpu(), s["scores"].cpu())]


def kutu_maskesi(img, kutular):
    """SAM 2.1: her kutudaki nesnenin maskesi (0/255, resim boyutunda). Gömme önbelleklenmez (her seferinde başka parça)."""
    import torch
    W, H = img.size
    with _kilit:
        sm, sp = _yukle("sam")
        dev = _cihaz()
        gi = sp(images=img.convert("RGB"), return_tensors="pt").to(dev)
        with torch.no_grad():
            emb = sm.get_image_embeddings(gi["pixel_values"])
            isl = sp(images=Image.new("RGB", (W, H)), input_boxes=[kutular], return_tensors="pt").to(dev)
            o = sm(input_boxes=isl["input_boxes"], image_embeddings=emb, multimask_output=False)
        mm = sp.post_process_masks(o.pred_masks.cpu(), isl["original_sizes"].cpu())[0].reshape(-1, H, W).numpy()
    return [(x > 0).astype(np.uint8) * 255 for x in mm]


# ------------------------------------------------------------------ SegFormer: sınıf maskeleri
def bolut(anahtar, img):
    """ADE20K sınıf haritası (uint8, resim boyutunda). Büyük resimde ~1 MP'de çalışıp büyütülür."""
    import torch
    with _kilit:
        hazir = _onbellek.get(("segformer", anahtar))
        if hazir is not None:
            return hazir
        m, p = _yukle("segformer")
        W, H = img.size
        k = min(1.0, (1.2e6 / (W * H)) ** 0.5)
        kucuk = img.convert("RGB").resize((max(32, int(W * k)), max(32, int(H * k))), Image.BICUBIC)
        girdi = p(images=kucuk, return_tensors="pt", do_resize=False).to(_cihaz())
        with torch.no_grad():
            logit = m(**girdi).logits
            logit = torch.nn.functional.interpolate(logit, size=(H, W), mode="bilinear", align_corners=False)
        harita = logit.argmax(1)[0].to(torch.uint8).cpu().numpy()
        _sakla("segformer", anahtar, harita)
        return harita


def sinif_maskesi(anahtar, img, grup, rafine=True):
    """gökyüzü / bitki / cam maskesi (0–255). rafine: resim kenarlarına oturtulmuş yumuşak maske (dal, yaprak)."""
    harita = bolut(anahtar, img)
    m = np.isin(harita, ADE[grup]).astype(np.float32)
    if rafine:
        m = _guided(np.asarray(img.convert("L"), np.float32) / 255.0, m, r=max(4, min(img.size) // 200), eps=1e-3)
    return (np.clip(m, 0, 1) * 255).astype(np.uint8)


def _guided(I, p, r, eps):
    """Kılavuzlu süzgeç (He ve ark.): maskeyi resmin ince kenarlarına (ağaç dalları) oturtur."""
    import cv2
    k = (2 * r + 1, 2 * r + 1)
    mI, mp = cv2.boxFilter(I, -1, k), cv2.boxFilter(p, -1, k)
    cov = cv2.boxFilter(I * p, -1, k) - mI * mp
    var = cv2.boxFilter(I * I, -1, k) - mI * mI
    a = cov / (var + eps)
    b = mp - a * mI
    return cv2.boxFilter(a, -1, k) * I + cv2.boxFilter(b, -1, k)


# ------------------------------------------------------------------ Depth Anything V2: derinlik
def derinlik(anahtar, img):
    """Göreli derinlik (float32, 0 = en uzak, 1 = en yakın), resim boyutunda."""
    import torch
    with _kilit:
        hazir = _onbellek.get(("derinlik", anahtar))
        if hazir is not None:
            return hazir
        m, p = _yukle("derinlik")
        W, H = img.size
        girdi = p(images=img.convert("RGB"), return_tensors="pt").to(_cihaz())
        with torch.no_grad():
            d = m(pixel_values=girdi["pixel_values"].half()).predicted_depth
            d = torch.nn.functional.interpolate(d.unsqueeze(1).float(), size=(H, W), mode="bicubic", align_corners=False)[0, 0]
        d = d.cpu().numpy()
        d = (d - d.min()) / max(1e-6, d.max() - d.min())
        _sakla("derinlik", anahtar, d.astype(np.float32))
        return _onbellek[("derinlik", anahtar)]
