# ============================================================
# Lisans Yonetim Aracı (generate / list / revoke)
# ============================================================
#
# Kullanim:
#   powershell -ExecutionPolicy Bypass -File scripts/license-cli.ps1 -Generate -Business "Musteri Kafe" -Count 2
#   powershell -ExecutionPolicy Bypass -File scripts/license-cli.ps1 -List
#   powershell -ExecutionPolicy Bypass -File scripts/license-cli.ps1 -Revoke -Key "JGJC-XXXXX-XXXXX-XXXXX"
#
# Not: Bu betik %LOCALAPPDATA%\oyun-kafe-yonetim\license-config.json
# icindeki admin_token ve service_key'i kullanir. Betigi GIT'e commit ETMEYIN.

param(
  [switch]$Generate,
  [string]$Business = "",
  [int]$Count = 1,
  [switch]$List,
  [switch]$Revoke,
  [string]$Key = ""
)

$ErrorActionPreference = "Stop"
$cfgPath = Join-Path $env:LOCALAPPDATA "oyun-kafe-yonetim\license-config.json"
if (-not (Test-Path $cfgPath)) { Write-Error "license-config.json bulunamadi. once deploy-license.ps1 calistirin."; exit 1 }
$cfg = Get-Content $cfgPath -Raw | ConvertFrom-Json
$api = $cfg.url
$adminH = @{ "x-admin-token" = $cfg.admin_token; "Content-Type" = "application/json" }
$svcH = @{ "apikey" = $cfg.service_key; "Authorization" = "Bearer $($cfg.service_key)"; "Content-Type" = "application/json" }

if ($Generate) {
  if (-not $Business) { $Business = Read-Host "Isletme adi (musterinin kafesi)" }
  $body = @{ business_name = $Business; count = $Count } | ConvertTo-Json
  $r = Invoke-RestMethod -Uri "$api/functions/v1/license/generate" -Headers $adminH -Method Post -Body $body -TimeoutSec 30
  Write-Host ""
  Write-Host "Uretilen lisanslar ($Business):"
  Write-Host "------------------------------------------"
  $r.keys | ForEach-Object { Write-Host ("  " + $_.key + "   [" + $_.license_id + "]") }
  Write-Host ""
  Write-Host "Musteriye JGJC-XXXXX-XXXXX-XXXXX formatinda anahtari verin."
  Write-Host "Musteri: Ayarlar -> Lisans -> anahtari yapistir -> Aktif Et"
  exit 0
}

if ($List) {
  $rows = Invoke-RestMethod -Uri "$api/rest/v1/licenses?select=key,license_id,business_name,status,machine_hash&order=created_at.asc" -Headers $svcH -Method Get -TimeoutSec 30
  if (-not $rows -or $rows.Count -eq 0) { Write-Host "Henuz lisans yok."; exit 0 }
  Write-Host "Lisanslar:"
  Write-Host ("{0,-24} {1,-10} {2,-22} {3}" -f "ANAHTAR", "DURUM", "ISLETME", "MAKINE")
  Write-Host "--------------------------------------------------------------------------"
  foreach ($x in $rows) {
    $m = ""
    if ($x.machine_hash) { $m = $x.machine_hash.Substring(0, 8) }
    Write-Host ("{0,-24} {1,-10} {2,-22} {3}" -f $x.key, $x.status, $x.business_name, $m)
  }
  exit 0
}

if ($Revoke) {
  if (-not $Key) { $Key = Read-Host "Iptal edilecek lisans anahtari" }
  $Key = $Key.Trim().ToUpper()
  $body = @{ status = "revoked" } | ConvertTo-Json
  $r = Invoke-RestMethod -Uri "$api/rest/v1/licenses?key=eq.$Key" -Headers $svcH -Method Patch -Body $body -TimeoutSec 30
  Write-Host "Iptal edildi: $Key"
  Write-Host "Musterinin uygulamasi en gec 6 saat icinde kilitlenecek (online kontrol)."
  exit 0
}

Write-Host "Kullanim: -Generate -Business `"Ad`" -Count N | -List | -Revoke -Key ANAHTAR"
