# ============================================================
# supabase.json Anahtar Rotasyon Betigi
# ============================================================
#
# supabase.json nedir?
#   %LOCALAPPDATA%\oyun-kafe-yonetim\supabase.json  (git'e commit EDILMEZ)
#   Masaustu uygulamanin Supabase baglanti bilgileridir:
#     url          : Supabase proje REST URL'si
#     anon_key     : Kisa sureli public JWT (mobil panel okur)
#     service_key  : SUPER YETKILI service_role JWT (desktop sync kullanir, ASLA paylasilmaz)
#     panel_url    : Mobil panelin canli adresi (ornek: https://panel-deploy-six.vercel.app)
#
# Ne zaman rotasyon gerekir?
#   - supabase.json sizdiyse veya yanlislikla bir yere commit edildiyse
#   - Eski anahtarlarin ele gecirildigi supheli bir durum varsa
#   - Rutin olarak (orn. 90 gunde bir)
#
# Adimlar:
#   1) Supabase Dashboard -> Project Settings -> API -> "Rotate JWT secret"
#      (Yeni anon + service_role anahtarlari olusur; eskileri gecersiz olur)
#   2) Bu betigi yeni anahtarlarla calistir
#   3) Masaustu uygulamayi YENIDEN baslat (eski anahtarla aciksa sync kesilir)
#   4) Mobil panel token'lari deploy betigi calisirken supabase.json'dan
#      dolduruldugu icin paneli yeniden deploy et:
#        powershell -ExecutionPolicy Bypass -File scripts/deploy-vercel.ps1 -Token "VERCEL_TOKEN"
#
# Kullanim (parametre veya ortam degiskenleri):
#   $env:SB_URL="https://xyz.supabase.co"
#   $env:SB_ANON_KEY="eyJ..."
#   $env:SB_SERVICE_KEY="eyJ..."
#   $env:SB_PANEL_URL="https://panel-deploy-six.vercel.app"
#   powershell -ExecutionPolicy Bypass -File scripts/rotate-supabase-keys.ps1
# ============================================================

param(
  [string]$Url = $env:SB_URL,
  [string]$AnonKey = $env:SB_ANON_KEY,
  [string]$ServiceKey = $env:SB_SERVICE_KEY,
  [string]$PanelUrl = $env:SB_PANEL_URL
)

$ErrorActionPreference = "Stop"

$configPath = Join-Path $env:LOCALAPPDATA "oyun-kafe-yonetim\supabase.json"

if (-not (Test-Path $configPath)) {
  Write-Error "supabase.json bulunamadi: $configPath"
  exit 1
}

$cfg = Get-Content $configPath -Raw | ConvertFrom-Json

if (-not $Url)    { $Url = $cfg.url }
if (-not $AnonKey) { $AnonKey = $cfg.anon_key }
if (-not $ServiceKey) { $ServiceKey = $cfg.service_key }
if (-not $PanelUrl) { $PanelUrl = if ($cfg.panel_url) { $cfg.panel_url } else { "" } }

if (-not $AnonKey -or -not $ServiceKey) {
  Write-Error "AnonKey ve ServiceKey zorunlu. Env degiskenlerini veya parametreleri gecin."
  exit 1
}

# Eski yapiyi yedekle
$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$backup = "$configPath.bak-$stamp"
Copy-Item -LiteralPath $configPath -Destination $backup
Write-Host "Yedek alindi: $backup"

$new = [ordered]@{
  url          = $Url
  anon_key     = $AnonKey
  service_key  = $ServiceKey
  panel_url    = $PanelUrl
}
$json = $new | ConvertTo-Json
[System.IO.File]::WriteAllText($configPath, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "supabase.json guncellendi."

Write-Host ""
Write-Host "SONRAKI ADIMLAR:"
Write-Host "  1) Masaustu uygulamayi kapatip yeniden baslatin (sync yeni anahtarla devam eder)."
Write-Host "  2) Mobil paneli yeniden deploy edin:"
Write-Host "     powershell -ExecutionPolicy Bypass -File scripts/deploy-vercel.ps1 -Token `"VERCEL_TOKEN`""
