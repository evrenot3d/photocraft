# EVREN eklentisi: PDF sayfalarını resme çevirir (PhotoCraft'ın "PDF İçe Aktar" penceresi kullanır).
# Windows'un kendi PDF çizicisi (Windows.Data.Pdf, Windows 10/11'de hazır) kullanılır: ek program indirilmez.
# Windows PowerShell 5.1 ile çalışır (WinRT erişimi için; pwsh 7 değil):
#   powershell -NoProfile -ExecutionPolicy Bypass -File pdf_ac.ps1 -Pdf <dosya> -Cikti <klasör> -Bilgi
#       -> her satır JSON: {"sayfa":1,"gen":595.3,"yuk":841.9}  (pt = 1/72 inç)  + küçük resimler k_<n>.png
#   ... -Sayfalar "1,3" -Dpi 300 [-Beyaz]
#       -> s_<n>.png (saydam arka plan; -Beyaz ile beyaz)
param(
    [Parameter(Mandatory = $true)][string]$Pdf,
    [Parameter(Mandatory = $true)][string]$Cikti,
    [switch]$Bilgi,
    [string]$Sayfalar = "",
    [double]$Dpi = 300,
    [int]$Kucuk = 220,
    [switch]$Beyaz
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
$null = [Windows.Data.Pdf.PdfDocument, Windows.Data.Pdf, ContentType = WindowsRuntime]
$null = [Windows.Storage.Streams.InMemoryRandomAccessStream, Windows.Storage.Streams, ContentType = WindowsRuntime]

$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
$asTaskAction = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' })[0]
function Bekle($islem, [Type]$tur) {
    $t = $asTaskGeneric.MakeGenericMethod($tur).Invoke($null, @($islem))
    $t.Wait() | Out-Null
    $t.Result
}
function BekleEylem($islem) {
    $t = $asTaskAction.Invoke($null, @($islem))
    $t.Wait() | Out-Null
}

$dosya = Bekle ([Windows.Storage.StorageFile]::GetFileFromPathAsync((Resolve-Path $Pdf).Path)) ([Windows.Storage.StorageFile])
$belge = Bekle ([Windows.Data.Pdf.PdfDocument]::LoadFromFileAsync($dosya)) ([Windows.Data.Pdf.PdfDocument])
New-Item -ItemType Directory -Force $Cikti | Out-Null
$klasor = Bekle ([Windows.Storage.StorageFolder]::GetFolderFromPathAsync((Resolve-Path $Cikti).Path)) ([Windows.Storage.StorageFolder])

Add-Type -AssemblyName System.Drawing
# Windows çizici ekran ölçeğiyle (ör. %175) büyütür: gerçek ölçek bir deneme çizimiyle ölçülür ve düzeltilir.
$script:oran = 0.0
function Ciz([int]$i, [string]$ad, [uint32]$gen, [uint32]$yuk, [bool]$beyazArka) {
    if ($script:oran -le 0) {
        CizHam $i $ad 400 400 $true
        $yol = Join-Path $klasor.Path $ad
        $im = [System.Drawing.Image]::FromFile($yol)
        $script:oran = [double]$im.Width / 400.0
        $im.Dispose()
        if ($script:oran -le 0.1) { $script:oran = 1.0 }
    }
    CizHam $i $ad ([uint32][Math]::Max(1, [Math]::Round($gen / $script:oran))) ([uint32][Math]::Max(1, [Math]::Round($yuk / $script:oran))) $beyazArka
}

function CizHam([int]$i, [string]$ad, [uint32]$gen, [uint32]$yuk, [bool]$beyazArka) {
    $sayfa = $belge.GetPage([uint32]$i)
    $sec = New-Object Windows.Data.Pdf.PdfPageRenderOptions
    $sec.DestinationWidth = $gen
    $sec.DestinationHeight = $yuk
    if ($beyazArka) {
        $sec.BackgroundColor = [Windows.UI.Color]::FromArgb(255, 255, 255, 255)
    } else {
        $sec.BackgroundColor = [Windows.UI.Color]::FromArgb(0, 255, 255, 255)
    }
    $hedef = Bekle ($klasor.CreateFileAsync($ad, [Windows.Storage.CreationCollisionOption]::ReplaceExisting)) ([Windows.Storage.StorageFile])
    $akis = Bekle ($hedef.OpenAsync([Windows.Storage.FileAccessMode]::ReadWrite)) ([Windows.Storage.Streams.IRandomAccessStream])
    try {
        BekleEylem ($sayfa.RenderToStreamAsync($akis, $sec))
    } finally {
        $akis.Dispose()
        $sayfa.Dispose()
    }
}

$n = [int]$belge.PageCount
if ($Bilgi) {
    for ($i = 0; $i -lt $n; $i++) {
        $s = $belge.GetPage([uint32]$i)
        # Size: DIP (1/96 inç) -> pt (1/72 inç)
        $g = [double]$s.Size.Width * 72.0 / 96.0
        $y = [double]$s.Size.Height * 72.0 / 96.0
        $s.Dispose()
        $k = [Math]::Min(1.0, $Kucuk / [Math]::Max($g, $y))
        Ciz $i ("k_{0}.png" -f ($i + 1)) ([uint32][Math]::Max(1, [Math]::Round($g * $k))) ([uint32][Math]::Max(1, [Math]::Round($y * $k))) $true
        '{{"sayfa":{0},"gen":{1},"yuk":{2}}}' -f ($i + 1), $g.ToString([Globalization.CultureInfo]::InvariantCulture), $y.ToString([Globalization.CultureInfo]::InvariantCulture)
    }
    exit 0
}
foreach ($p in ($Sayfalar -split ',' | Where-Object { $_.Trim() -ne '' })) {
    $i = [int]$p.Trim() - 1
    if ($i -lt 0 -or $i -ge $n) { continue }
    $s = $belge.GetPage([uint32]$i)
    $g = [double]$s.Size.Width / 96.0 * $Dpi
    $y = [double]$s.Size.Height / 96.0 * $Dpi
    $s.Dispose()
    Ciz $i ("h_{0}.png" -f ($i + 1)) ([uint32][Math]::Max(1, [Math]::Round($g))) ([uint32][Math]::Max(1, [Math]::Round($y))) ([bool]$Beyaz)
    # Çözünürlük (ppi) PNG'ye yazılır: belge Photoshop'taki gibi seçilen ppi ile açılsın.
    $ham = Join-Path $klasor.Path ("h_{0}.png" -f ($i + 1))
    $son = Join-Path $klasor.Path ("s_{0}.png" -f ($i + 1))
    $bmp = New-Object System.Drawing.Bitmap($ham)
    try {
        $bmp.SetResolution([single]$Dpi, [single]$Dpi)
        $bmp.Save($son, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $bmp.Dispose()
    }
    Remove-Item $ham -Force
    '{{"sayfa":{0},"dosya":"s_{0}.png","gen":{1},"yuk":{2}}}' -f ($i + 1), [Math]::Round($g), [Math]::Round($y)
}
