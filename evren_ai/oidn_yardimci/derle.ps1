# EVREN eklentisi: evren_oidn.exe'yi derler ve OIDN DLL'lerinin yanına (evren_ai\oidn\bin) kopyalar.
# OIDN DLL'leri: https://github.com/RenderKit/oidn/releases (oidn-<sürüm>.x64.windows.zip, bin klasörü).
$ErrorActionPreference = 'Stop'
$burasi = $PSScriptRoot
$hedef = Join-Path (Split-Path $burasi -Parent) 'oidn\bin'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
& $cargo build --release --manifest-path (Join-Path $burasi 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw "derleme başarısız" }
New-Item -ItemType Directory -Force $hedef | Out-Null
Copy-Item (Join-Path $burasi 'target\release\evren_oidn.exe') $hedef -Force
Write-Host "evren_oidn.exe -> $hedef"
