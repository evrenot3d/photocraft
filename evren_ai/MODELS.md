# AI models used by the PhotoCraft Evren add-ons

`install.ps1` downloads these from their official pages (`indir.py`). They are **not** part of this
repository and are **not** covered by its MIT / Apache-2.0 license: each keeps its own license. Several
are licensed for **non-commercial use only**; check the license on the model page before using results
commercially.

| Model | Used by | Size | Source | License (check the page) |
|---|---|---|---|---|
| Juggernaut XL v9 (RunDiffusion Photo v2) | Upscale (SUPIR), Fix People | 7.1 GB | [RunDiffusion/Juggernaut-XL-v9](https://huggingface.co/RunDiffusion/Juggernaut-XL-v9) | CreativeML Open RAIL++-M |
| SUPIR v0Q (fp16) | Upscale (SUPIR) | 2.7 GB | [Kijai/SUPIR_pruned](https://huggingface.co/Kijai/SUPIR_pruned) · [SUPIR](https://github.com/Fanghua-Yu/SUPIR) | SUPIR license (non-commercial) |
| 4x-UltraSharp | Upscale (fast), Fix People | 67 MB | [Kim2091/UltraSharp](https://huggingface.co/Kim2091/UltraSharp) | CC BY-NC-SA 4.0 |
| ControlNet Union SDXL 1.0 promax | Fix People (keeps the pose) | 2.5 GB | [xinsir/controlnet-union-sdxl-1.0](https://huggingface.co/xinsir/controlnet-union-sdxl-1.0) | Apache-2.0 |
| Grounding DINO base | Fix People, Separate Glass / Plants | 0.9 GB | [IDEA-Research/grounding-dino-base](https://huggingface.co/IDEA-Research/grounding-dino-base) | Apache-2.0 |
| SAM 2.1 base+ | Fix People, Separate Glass / Plants | 0.3 GB | [facebook/sam2.1-hiera-base-plus](https://huggingface.co/facebook/sam2.1-hiera-base-plus) | Apache-2.0 |
| SegFormer-B5 (ADE20K) | Replace Sky, Separate Plants / Glass | 0.3 GB | [nvidia/segformer-b5-finetuned-ade-640-640](https://huggingface.co/nvidia/segformer-b5-finetuned-ade-640-640) | NVIDIA SegFormer license (research / non-commercial) |
| Depth Anything V2 Large | Depth of Field, Fix People | 1.3 GB | [depth-anything/Depth-Anything-V2-Large-hf](https://huggingface.co/depth-anything/Depth-Anything-V2-Large-hf) | CC BY-NC 4.0 |
| Intel Open Image Denoise 2.5.1 | Denoise | 54 MB | [RenderKit/oidn](https://github.com/RenderKit/oidn/releases/tag/v2.5.1) | Apache-2.0 |

Software installed alongside: [ComfyUI](https://github.com/comfyanonymous/ComfyUI) (GPL-3.0, run as a separate
process only while an AI tool works), [ComfyUI-SUPIR](https://github.com/kijai/ComfyUI-SUPIR) and PyTorch.

The **Replace Sky** library (`gokyuzu_kutuphanesi/`) starts empty: add your own sky photos with the tool's
"Add Sky…" button.
