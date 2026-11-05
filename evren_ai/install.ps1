# PhotoCraft Evren — installs the AI add-ons (Upscale, Fix People, Depth of Field, Separate Plants / Glass,
# Replace Sky, Denoise). Run once from this folder:
#
#     powershell -ExecutionPolicy Bypass -File install.ps1
#
# Needs: Windows 10/11 x64, an NVIDIA graphics card (RTX, 8 GB or more recommended), Python 3.11 and Git,
# and about 35 GB of free disk space (≈22 GB of models + Python packages). Safe to run again: finished
# steps are skipped and interrupted downloads resume.
$ErrorActionPreference = 'Stop'
$kok = $PSScriptRoot
Set-Location $kok

$COMFY_REPO = 'https://github.com/comfyanonymous/ComfyUI.git'
$COMFY_COMMIT = '6e04cb20feea225a7cf72f946f6b616ffac35855'
$SUPIR_REPO = 'https://github.com/kijai/ComfyUI-SUPIR.git'
$SUPIR_COMMIT = '99d49e912c905ce1aaf7d15898f550b40fb3e6cc'
$TORCH_INDEX = 'https://download.pytorch.org/whl/cu128'

function Adim($m) { Write-Host "`n== $m" -ForegroundColor Cyan }

# 1. Python 3.11 and Git
Adim 'Checking Python 3.11 and Git'
$py = $null
try { $py = (& py -3.11 -c "import sys; print(sys.executable)") 2>$null } catch {}
if (-not $py) {
    throw 'Python 3.11 not found. Install it from https://www.python.org/downloads/release/python-3119/ (tick "Add to PATH"), then run this script again.'
}
try { git --version | Out-Null } catch { throw 'Git not found. Install it from https://git-scm.com/download/win, then run this script again.' }

# 2. Virtual environment
$venvPy = Join-Path $kok '.venv\Scripts\python.exe'
if (-not (Test-Path $venvPy)) {
    Adim 'Creating the Python environment (.venv)'
    & $py -m venv (Join-Path $kok '.venv')
}
& $venvPy -m pip install --upgrade pip --quiet

# 3. PyTorch with CUDA
Adim 'Installing PyTorch (CUDA 12.8, about 3 GB)'
& $venvPy -m pip install torch==2.11.0 torchvision==0.26.0 torchaudio==2.11.0 --index-url $TORCH_INDEX
if ($LASTEXITCODE -ne 0) { throw 'PyTorch could not be installed.' }

# 4. ComfyUI (the AI engine, started only while a tool runs)
$comfy = Join-Path $kok 'ComfyUI'
if (-not (Test-Path (Join-Path $comfy 'main.py'))) {
    Adim 'Downloading ComfyUI'
    git clone $COMFY_REPO $comfy
}
git -C $comfy fetch --quiet origin
git -C $comfy checkout --quiet $COMFY_COMMIT
& $venvPy -m pip install -r (Join-Path $comfy 'requirements.txt')

$supir = Join-Path $comfy 'custom_nodes\ComfyUI-SUPIR'
if (-not (Test-Path $supir)) {
    Adim 'Downloading the SUPIR node for ComfyUI'
    git clone $SUPIR_REPO $supir
}
git -C $supir fetch --quiet origin
git -C $supir checkout --quiet $SUPIR_COMMIT

# 5. Add-on packages
Adim 'Installing the add-on packages'
& $venvPy -m pip install -r (Join-Path $kok 'requirements.txt')
if ($LASTEXITCODE -ne 0) { throw 'The add-on packages could not be installed.' }

# 6. Models (~22 GB) and the denoiser
Adim 'Downloading the AI models (about 22 GB; this takes a while)'
& $venvPy (Join-Path $kok 'indir.py')
if ($LASTEXITCODE -ne 0) { throw 'A model download failed; run the script again to resume.' }

# 7. Denoise helper (needs Rust; official PhotoCraft Evren releases ship it already built)
$oidnExe = Join-Path $kok 'oidn\bin\evren_oidn.exe'
if (-not (Test-Path $oidnExe)) {
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (Test-Path $cargo) {
        Adim 'Building the Denoise helper'
        & (Join-Path $kok 'oidn_yardimci\derle.ps1')
    } else {
        Write-Host 'Denoise helper not built (Rust not installed). Install Rust from https://rustup.rs and run oidn_yardimci\derle.ps1, or use a release build.' -ForegroundColor Yellow
    }
}

New-Item -ItemType Directory -Force (Join-Path $kok 'gokyuzu_kutuphanesi') | Out-Null
Write-Host "`nDone. Start PhotoCraft Evren; the AI buttons are at the bottom of the left toolbar." -ForegroundColor Green
Write-Host 'Replace Sky: put your own sky photos in evren_ai\gokyuzu_kutuphanesi (or use "Add Sky..." in the tool).'
