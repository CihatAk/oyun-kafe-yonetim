# Oyun Kafe Mobil Panel - Vercel deploy betigi
# 1) src-tauri/web/index.html'daki token'lari doldurur
# 2) panel-deploy/ klasorune (index.html + vercel.json) yazar
# 3) vercel CLI ile production deploy eder
#
# Gereksinimler:
#   - vercel CLI kurulu (npm install -g vercel)
#   - Vercel hesabi ve token: https://vercel.com/account/tokens (Create Token)
#   - Token: -Token parametresi veya VERCEL_TOKEN cevre degiskeni
#
# Kullanim:
#   powershell -ExecutionPolicy Bypass -File scripts/deploy-vercel.ps1 -Token "xxxx"

param([string]$Token = $env:VERCEL_TOKEN)

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $env:LOCALAPPDATA "oyun-kafe-yonetim\supabase.json"
$sourceHtml = Join-Path $repo "src-tauri\web\index.html"
$deployDir = Join-Path $repo "panel-deploy"

if (-not (Test-Path $configPath)) { Write-Error "supabase.json bulunamadi: $configPath"; exit 1 }
if (-not (Test-Path $sourceHtml)) { Write-Error "index.html bulunamadi: $sourceHtml"; exit 1 }
if (-not $Token) {
  Write-Error "Vercel token gerekli. https://vercel.com/account/tokens adresinden token olusturun."
  exit 1
}

$cfg = Get-Content $configPath -Raw | ConvertFrom-Json
if (-not $cfg.url -or -not $cfg.anon_key) { Write-Error "supabase.json icinde url/anon_key eksik."; exit 1 }

$html = [System.IO.File]::ReadAllText($sourceHtml, [System.Text.Encoding]::UTF8)
$html = $html.Replace("__SUPABASE_URL__", $cfg.url).Replace("__SUPABASE_ANON_KEY__", $cfg.anon_key)
if ($html.Contains("__SUPABASE_URL__") -or $html.Contains("__SUPABASE_ANON_KEY__")) {
  Write-Error "Token degistirilemedi (index.html icerigini kontrol edin)."; exit 1
}

if (Test-Path $deployDir) { Remove-Item -LiteralPath $deployDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $deployDir | Out-Null
[System.IO.File]::WriteAllText((Join-Path $deployDir "index.html"), $html, (New-Object System.Text.UTF8Encoding($false)))
Set-Content -Path (Join-Path $deployDir "vercel.json") -Value '{"cleanUrls": false}' -Encoding ascii

Write-Host "Deploy dosyalari hazir: $deployDir"
Write-Host "Vercel deploy basliyor..."
& vercel deploy $deployDir --prod --yes --token $Token
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
