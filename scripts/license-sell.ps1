# ============================================================
# Lisans Satis Aracı (tek komut: kayit + anahtar + musteri metni)
# ============================================================
#
# Kullanim:
#   powershell -ExecutionPolicy Bypass -File scripts/license-sell.ps1 -Business "Kafe Adi"
#   powershell -ExecutionPolicy Bypass -File scripts/license-sell.ps1 -Business "Kafe Adi" -Count 3
#   powershell -ExecutionPolicy Bypass -File scripts/license-sell.ps1 -Business "Kafe Adi" -Days 365
#
# -Days: abonelik suresi (varsayilan 0 = suresiz). 0'dan buyukse
#         musteriye süre bilgisi de yazilir.
# -Count: tek seferde uretilecek anahtar sayisi (varsayilan 1).
#
# Bu betik %LOCALAPPDATA%\oyun-kafe-yonetim\license-config.json
# icindeki admin_token'i kullanir. Betigi GIT'e commit ETMEYIN.
# ============================================================

param(
  [string]$Business = "",
  [int]$Count = 1,
  [int]$Days = 0,
  [switch]$NoClipboard
)

$ErrorActionPreference = "Stop"
$cfgPath = Join-Path $env:LOCALAPPDATA "oyun-kafe-yonetim\license-config.json"
if (-not (Test-Path $cfgPath)) { Write-Error "license-config.json bulunamadi. once deploy-license.ps1 calistirin."; exit 1 }

$cfg = Get-Content $cfgPath -Raw | ConvertFrom-Json
$api = $cfg.url

if (-not $Business) { $Business = Read-Host "Isletme adi (musterinin kafesi)" }
$Business = $Business.Trim()
if (-not $Business) { Write-Error "Isletme adi bos olamaz."; exit 1 }

$body = @{ business_name = $Business; count = $Count } | ConvertTo-Json
if ($Days -gt 0) {
  $expiresAt = (Get-Date).AddDays($Days).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  $body = @{ business_name = $Business; count = $Count; expires_at = $expiresAt } | ConvertTo-Json
}
$headers = @{ "x-admin-token" = $cfg.admin_token; "Content-Type" = "application/json" }

$r = Invoke-RestMethod -Uri "$api/functions/v1/license/generate" -Headers $headers -Method Post -Body $body -TimeoutSec 30

$today = Get-Date
$expiryText = "suresiz"
if ($Days -gt 0) {
  $expiry = $today.AddDays($Days)
  $expiryText = "$($expiry.ToString("dd.MM.yyyy")) tarihine kadar"
}

Write-Host ""
Write-Host "=========================================================="
Write-Host " JIJI GAME CENTER - LISANS KAYDI"
Write-Host "=========================================================="
Write-Host " Isletme : $Business"
Write-Host " Tarih   : $($today.ToString("dd.MM.yyyy HH:mm"))"
Write-Host " Gecerlilik: $expiryText"
Write-Host " Anahtar : "
$r.keys | ForEach-Object {
  Write-Host "     $($_.key)   [$($_.license_id)]"
}
Write-Host "=========================================================="
Write-Host ""
Write-Host "MUSTERIYE ILETILECEK METIN:"
Write-Host "------------------------------------------"
foreach ($k in $r.keys) {
  $periodText = if ($Days -gt 0) { "Lisansiniz $expiryText gecerlidir." } else { "Lisansiniz suresiz gecerlidir." }
  $customerText = @"
Sayin yetkili,

JiJi Game Center yazilim lisansiniz hazirdir:

  Lisans Anahtari: $($k.key)

Uygulamayi acin, sol ust menuden 'Ayarlar' sekmesine gidin,
'Lisans' bolumune bu anahtari yapistirin ve 'Aktif Et' butonuna basin.

$periodText
Anahtariniz yalnizca bu isletmenin kayitli bilgisayarinda gecerlidir.

Iyi calismalar,
JiJi Game Center
"@
  Write-Host $customerText
  Write-Host "------------------------------------------"

  if (-not $NoClipboard -and $r.keys.Count -eq 1) {
    try {
      Set-Clipboard -Value $k.key
      Write-Host "(Anahtar panoya kopyalandi)"
    } catch { }
  }
}
Write-Host ""

# JSON cikti istersen (otomasyon icin):
#   .\scripts\license-sell.ps1 -Business "X" | Out-File -Encoding utf8 kayit.txt
