# PhotoCraft resmi güncellemesini EVREN dalına alır ve derler.
# Kullanım:  .\EVREN_guncelle.ps1            (en yeni resmi sürüm etiketi)
#            .\EVREN_guncelle.ps1 -Surum v0.21.0
param([string]$Surum = "")

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

$dal = (git rev-parse --abbrev-ref HEAD).Trim()
if ($dal -ne "evren") { Write-Host "Önce 'evren' dalına geçin (şu an: $dal)." -ForegroundColor Red; exit 1 }
if (git status --porcelain) { Write-Host "Kaydedilmemiş (commit'lenmemiş) değişiklik var; önce commit edin." -ForegroundColor Red; git status --short; exit 1 }

Write-Host "Resmi depodan güncellemeler alınıyor..." -ForegroundColor Cyan
git fetch upstream --tags --force
if (-not $Surum) {
    $Surum = (git tag --list "v*" --sort=-v:refname | Select-Object -First 1).Trim()
}
Write-Host "Birleştirilecek sürüm: $Surum" -ForegroundColor Cyan
git merge --no-edit $Surum
if ($LASTEXITCODE -ne 0) {
    Write-Host "ÇAKIŞMA var. Şu dosyalar elle düzeltilmeli (EVREN eklentisi satırlarını koruyun):" -ForegroundColor Yellow
    git diff --name-only --diff-filter=U
    exit 1
}
Write-Host "Derleniyor (uzun sürebilir)..." -ForegroundColor Cyan
cargo build --release -p photocraft
if ($LASTEXITCODE -ne 0) { Write-Host "Derleme hatası." -ForegroundColor Red; exit 1 }
Write-Host "Tamam: $Surum + EVREN eklentileri derlendi (target\release\photocraft.exe)." -ForegroundColor Green
