# ============================================================
# Lisans Bilgileri Yedekleme Betigi
# ============================================================
#
# license-config.json (admin_token, service_key, db_password) ve
# NOTES.md (kılavuz) dosyalarını istedigin güvenli klasore kopyalar.
#
# Kullanim:
#   powershell -ExecutionPolicy Bypass -File scripts\yedekle-license.ps1 -Hedef "D:\LisansYedegi"
#
# NOT: Yedekler GIZLIDIR. USB disk / sifreli klasor / bulut depolama gibi
#      yalniz senin ulasabildigin bir yere kopyala. GIT'e atmA.
# ============================================================

param(
  [Parameter(Mandatory = $true)]
  [string]$Hedef
)

$ErrorActionPreference = "Stop"

$srcDir = Join-Path $env:LOCALAPPDATA "oyun-kafe-yonetim"
$cfgPath = Join-Path $srcDir "license-config.json"
$notesPath = "NOTES.md"   # calisma klasorunde

if (-not (Test-Path $cfgPath)) { Write-Error "license-config.json bulunamadi: $cfgPath"; exit 1 }
if (-not (Test-Path $notesPath)) {
  Write-Warning "NOTES.md calisma klasorunde yok ($PWD) - sadece config yedeklenecek."
  $notesPath = $null
}

New-Item -ItemType Directory -Path $Hedef -Force | Out-Null

$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$sub = Join-Path $Hedef "oyun-kafe-lisans"
New-Item -ItemType Directory -Path $sub -Force | Out-Null

Copy-Item -LiteralPath $cfgPath -Destination (Join-Path $sub "license-config.json") -Force
if ($notesPath) { Copy-Item -LiteralPath $notesPath -Destination (Join-Path $sub "NOTES.md") -Force }

Write-Host ""
Write-Host "Yedeklendi: $sub"
Get-ChildItem $sub | ForEach-Object { Write-Host ("  - " + $_.Name + "  (" + $_.Length + " bayt)") }
Write-Host ""
Write-Host "ONEMLI: Bu yedegin SIFRELI oldugundan emin ol (bitlocker / sifreli klasor)."
