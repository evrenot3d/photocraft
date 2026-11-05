"""PhotoCraft Evren — downloads the AI models the add-ons use (called by install.ps1).

Every file comes from its official Hugging Face / GitHub page (see MODELS.md for sources and licenses).
Files already present with the right size are skipped, so the script can be re-run to resume.

  python indir.py            # all models (~22 GB)
  python indir.py --list     # what would be downloaded
"""
import os
import shutil
import sys
import urllib.request
import zipfile

KOK = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, KOK)
MODELLER = os.path.join(KOK, "ComfyUI", "models")

# (folder under ComfyUI/models, file name, Hugging Face repo, file in the repo, size in bytes, used by)
COMFY = [
    ("checkpoints", "Juggernaut-XL_v9_RunDiffusionPhoto_v2.safetensors", "RunDiffusion/Juggernaut-XL-v9",
     "Juggernaut-XL_v9_RunDiffusionPhoto_v2.safetensors", 7105348188, "Upscale (SUPIR), Fix People"),
    ("checkpoints", "SUPIR-v0Q_fp16.safetensors", "Kijai/SUPIR_pruned", "SUPIR-v0Q_fp16.safetensors", 2664858464, "Upscale (SUPIR)"),
    ("upscale_models", "4x-UltraSharp.safetensors", "Kim2091/UltraSharp", "4x-UltraSharp.safetensors", 66864028, "Upscale (fast), Fix People"),
    ("controlnet", "controlnet-union-sdxl-1.0-promax.safetensors", "xinsir/controlnet-union-sdxl-1.0",
     "diffusion_pytorch_model_promax.safetensors", 2513342408, "Fix People (pose)"),
]

OIDN_URL = "https://github.com/RenderKit/oidn/releases/download/v2.5.1/oidn-2.5.1.x64.windows.zip"
OIDN_DLL = ["OpenImageDenoise.dll", "OpenImageDenoise_core.dll", "OpenImageDenoise_device_cpu.dll",
            "OpenImageDenoise_device_cuda.dll", "tbb12.dll", "tbbbind.dll", "tbbbind_2_0.dll", "tbbbind_2_5.dll"]


def _hf(repo, dosya, hedef, boyut):
    if os.path.isfile(hedef) and (not boyut or os.path.getsize(hedef) == boyut):
        print(f"  ok      {os.path.relpath(hedef, KOK)}")
        return
    from huggingface_hub import hf_hub_download
    print(f"  indir   {repo}/{dosya}  ({boyut / 1e9:.2f} GB)" if boyut else f"  indir   {repo}/{dosya}")
    os.makedirs(os.path.dirname(hedef), exist_ok=True)
    gecici = hf_hub_download(repo_id=repo, filename=dosya, local_dir=os.path.join(KOK, "calisma", "indirme"))
    shutil.move(gecici, hedef)


def comfy_modelleri():
    for klasor, ad, repo, dosya, boyut, _ in COMFY:
        _hf(repo, dosya, os.path.join(MODELLER, klasor, ad), boyut)


def gorus_modelleri():
    import gorus
    for anahtar, m in gorus.MODELLER.items():
        for d in m["dosyalar"]:
            _hf(d["repo"], d["dosya"], os.path.join(MODELLER, d["klasor"].replace("/", os.sep), d["dosya"]), d.get("boyut"))


def oidn():
    hedef = os.path.join(KOK, "oidn", "bin")
    if all(os.path.isfile(os.path.join(hedef, d)) for d in OIDN_DLL):
        print("  ok      oidn/bin (Intel Open Image Denoise)")
        return
    os.makedirs(hedef, exist_ok=True)
    zip_yolu = os.path.join(KOK, "calisma", "oidn.zip")
    os.makedirs(os.path.dirname(zip_yolu), exist_ok=True)
    print(f"  indir   {OIDN_URL}")
    urllib.request.urlretrieve(OIDN_URL, zip_yolu)
    with zipfile.ZipFile(zip_yolu) as z:
        for n in z.namelist():
            ad = os.path.basename(n)
            if ad in OIDN_DLL or ad == "LICENSE.txt":
                with z.open(n) as k, open(os.path.join(hedef if ad != "LICENSE.txt" else os.path.dirname(hedef), ad), "wb") as h:
                    shutil.copyfileobj(k, h)
    os.remove(zip_yolu)


def main():
    if "--list" in sys.argv:
        for klasor, ad, repo, dosya, boyut, kim in COMFY:
            print(f"{boyut / 1e9:6.2f} GB  {repo}/{dosya}  ->  models/{klasor}/{ad}   ({kim})")
        print("  + Grounding DINO, SAM 2.1, SegFormer-B5, Depth Anything V2 (~2.9 GB) and Intel Open Image Denoise (54 MB)")
        return
    print("AI models (ComfyUI):")
    comfy_modelleri()
    print("Vision models (find people, plants, glass, sky, depth):")
    gorus_modelleri()
    print("Denoiser:")
    oidn()
    print("Done.")


if __name__ == "__main__":
    main()
