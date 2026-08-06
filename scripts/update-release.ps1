# JiJi Game Center - PlayStation & VR - Güncelleme Release Script
# Bu script yeni versiyon için gerekli dosyaları hazırlar ve GitHub'a yüklemeyi kolaylaştırır
#
# Kullanim:
#   powershell -ExecutionPolicy Bypass -File scripts/update-release.ps1 -Version "2.0.1"
#
# NOT: latest.json'in gerçek imza içermesi icin build, Tauri updater anahtariyla
# imzalanmalidir. Anahtar kalici güvenli konumda:
#   %LOCALAPPDATA%\oyun-kafe-yonetim\jiji-updater-2.key
# ve sifre sistem ortam degiskeninde olmali:
#   $env:TAURI_KEY_PASSWORD = "<anahtar sifresi>"

param([string]$Version = $env:RELEASE_VERSION)

$ErrorActionPreference = "Stop"

# Imzalama icin anahtari kalici güvenli konumdan yukle (TAURI_PRIVATE_KEY tanimli degilse)
if (-not $env:TAURI_PRIVATE_KEY) {
  $secureKey = Join-Path $env:LOCALAPPDATA "oyun-kafe-yonetim\jiji-updater-2.key"
  if (Test-Path $secureKey) {
    $env:TAURI_PRIVATE_KEY = (Get-Content $secureKey -Raw).Trim()
    Write-Host "TAURI_PRIVATE_KEY kalici konumdan yuklendi: $secureKey"
  } else {
    Write-Warning "TAURI_PRIVATE_KEY tanimli degil ve kalici anahtar bulunamadi: $secureKey"
  }
}

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

# 2. Build yap (updater imzali dosyalar uretmesi icin gizli anahtar cikarilmalidir)
if (-not $env:TAURI_PRIVATE_KEY) {
  Write-Warning "TAURI_PRIVATE_KEY tanimli degil! latest.json imzasiz uretilecek. Gizli anahtari secmeden devam ediliyor..."
}
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

# 4. latest.json + imzali guncelleme paketini kopyala (updater aktifse uretilir)
$bundleLatest = Join-Path $bundleDir "latest.json"
$nsisZip = Join-Path $bundleDir "nsis\oyun-kafe-yonetim_${Version}_x64-setup.nsis.zip"
$nsisSig = Join-Path $bundleDir "nsis\oyun-kafe-yonetim_${Version}_x64-setup.nsis.zip.sig"
$msiZip = Join-Path $bundleDir "msi\oyun-kafe-yonetim_${Version}_x64_en-US.msi.zip"
$msiSig = Join-Path $bundleDir "msi\oyun-kafe-yonetim_${Version}_x64_en-US.msi.zip.sig"

if (Test-Path $nsisZip) { Copy-Item $nsisZip $releaseDir }
if (Test-Path $nsisSig) { Copy-Item $nsisSig $releaseDir }
if (Test-Path $msiZip) { Copy-Item $msiZip $releaseDir }
if (Test-Path $msiSig) { Copy-Item $msiSig $releaseDir }

# latest.json'i uret (build uretmez; imzadan uretiliyor)
$signature = $null
if (Test-Path $nsisSig) {
  $signature = (Get-Content $nsisSig -Raw).Trim()
} elseif (Test-Path $msiSig) {
  $signature = (Get-Content $msiSig -Raw).Trim()
}
if ($signature) {
  $notes = "v$Version"
  $pubDate = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  $url = "https://github.com/CihatAk/oyun-kafe-yonetim/releases/download/v$Version/oyun-kafe-yonetim_${Version}_x64-setup.nsis.zip"
  $latestJson = [ordered]@{
    version   = $Version
    notes     = $notes
    pub_date  = $pubDate
    platforms = [ordered]@{
      "windows-x86_64" = [ordered]@{
        signature = $signature
        url       = $url
      }
    }
  } | ConvertTo-Json -Depth 5
  Set-Content -Path $bundleLatest -Value $latestJson -NoNewline -Encoding UTF8
  Copy-Item $bundleLatest $releaseDir
  Write-Host "latest.json olusturuldu: $releaseDir\latest.json"
} else {
  Write-Warning "Imza bulunamadi; latest.json uretilemedi. Build'in imzali oldugundan emin olun (TAURI_PRIVATE_KEY + TAURI_KEY_PASSWORD)."
}

Write-Host ""
Write-Host "Sıradaki adımlar:"
Write-Host "1. $releaseDir klasörünü kontrol edin"
Write-Host "2. GitHub'a v$Version tag ile release oluşturun"
Write-Host "3. Setup.exe, msi, nsis.zip, nsis.zip.sig ve latest.json'i release'e yükleyin"
