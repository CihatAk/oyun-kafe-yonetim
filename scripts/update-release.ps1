# JiJi Game Center - PlayStation & VR - Güncelleme Release Script
# Bu script yeni versiyon için gerekli dosyaları hazırlar ve GitHub'a yüklemeyi kolaylaştırır
#
# Kullanim:
#   powershell -ExecutionPolicy Bypass -File scripts/update-release.ps1 -Version "2.0.1"

param([string]$Version = $env:RELEASE_VERSION)

$ErrorActionPreference = "Stop"

if (-not $Version) {
  Write-Error "Version parametresi gerekli. Ornek: -Version '2.0.1'"
  exit 1
}

$repo = Split-Path -Parent $PSScriptRoot
$srcTauri = Join-Path $repo "src-tauri"

Write-Host "Version: $Version"
Write-Host "Repo: $repo"

# 1. Cargo.toml'da version güncelle (yalnızca [package] blokundaki satır)
$cargoToml = Join-Path $srcTauri "Cargo.toml"
$cargoContent = Get-Content $cargoToml -Raw
$cargoContent = $cargoContent -replace '(?m)^(name = "oyun-kafe-yonetim"\r?\n)version = "[^"]*"', ('${1}version = "' + $Version + '"')
Set-Content -Path $cargoToml -Value $cargoContent -NoNewline
Write-Host "Cargo.toml version güncellendi"

# 2. Build yap
Write-Host "Build başlıyor..."
Push-Location $srcTauri
cargo tauri build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Pop-Location
Write-Host "Build tamamlandı"

# 3. Setup dosyalarını kopyala
$bundleDir = Join-Path $srcTauri "target\release\bundle"
$releaseDir = Join-Path $repo "release-$Version"
if (Test-Path $releaseDir) { Remove-Item -LiteralPath $releaseDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null

Copy-Item (Join-Path $bundleDir "nsis\oyun-kafe-yonetim_${Version}_x64-setup.exe") $releaseDir
Copy-Item (Join-Path $bundleDir "msi\oyun-kafe-yonetim_${Version}_x64_en-US.msi") $releaseDir
Write-Host "Setup dosyaları kopyalandı: $releaseDir"

# 4. latest.json hazırla
$latestJson = @{
  version = $Version
  notes = "JiJi Game Center - PlayStation & VR $Version güncellemesi"
  pub_date = (Get-Date).ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = @{
    "windows-x86_64" = @{
      signature = ""
      url = "https://github.com/CihatAk/oyun-kafe-yonetim/releases/download/v$Version/oyun-kafe-yonetim_${Version}_x64-setup.exe"
    }
  }
}
$latestJson | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $releaseDir "latest.json") -Encoding UTF8
Write-Host "latest.json oluşturuldu"

Write-Host ""
Write-Host "Sıradaki adımlar:"
Write-Host "1. $releaseDir klasörünü kontrol edin"
Write-Host "2. GitHub'a v$Version tag ile release oluşturun"
Write-Host "3. Setup dosyalarını ve latest.json'i release'e yükleyin"
Write-Host "4. Setup dosyasını imzalamak için: cargo tauri signer sign <dosya>"
