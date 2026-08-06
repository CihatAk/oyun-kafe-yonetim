# ============================================================
# Lisans Sistemi Deploy Betigi (Supabase edge function)
# ============================================================
#
# Bu betik lisans sunucusunu Supabase'e kurar ve anahtarlari
# dogru yerlere yazar. Adimlar:
#   1) Gerekirse Ed25519 anahtar cifti uretir (scripts/gen-license-keys.mjs)
#   2) Edge function env'lerine LICENSE_PRIVATE_KEY + LICENSE_ADMIN_TOKEN koyar
#   3) supabase functions deploy license ile fonksiyonu yayinlar
#   4) Public key'i ve sunucu adresini masaustu uygulamanin
#      kaynak sabitlerine yazar (LICENSE_PUBLIC_KEY / LICENSE_SERVER_URL / LICENSE_ANON_KEY)
#
# NOT: supabase CLI kurulu ve giris yapilmis olmali:
#       npm install -g supabase; supabase login
#
# Kullanim (parametre veya ortam degiskenleri):
#   $env:SB_URL="https://xyz.supabase.co"
#   $env:SB_ANON_KEY="eyJ..."
#   $env:LICENSE_ADMIN_TOKEN="rastgele-gizli-token"
#   powershell -ExecutionPolicy Bypass -File scripts/deploy-license.ps1
#
# LICENSE_ADMIN_TOKEN: Lisans cikarma/generate ucu icin kullanilir.
#   Supabase dashboard'dan her islemde girmek yerine en az 32 karakterlik
#   rastgele bir deger uretin: node -e "console.log(require('crypto').randomBytes(24).toString('hex'))"
# ============================================================

param(
  [string]$Url = $env:SB_URL,
  [string]$AnonKey = $env:SB_ANON_KEY,
  [string]$AdminToken = $env:LICENSE_ADMIN_TOKEN
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$licenseFile = Join-Path $root "src-tauri\src\license.rs"

# ─── 0) supabase CLI kontrol ─────────────────────────────────
if (-not (Get-Command supabase -ErrorAction SilentlyContinue)) {
  Write-Error "supabase CLI bulunamadi. Kurun: npm install -g supabase"
  exit 1
}

# ─── 1) Baglanti bilgileri (supabase.json'dan veya parametre) ─
$configPath = Join-Path $env:LOCALAPPDATA "oyun-kafe-yonetim\supabase.json"
if (Test-Path $configPath) {
  $cfg = Get-Content $configPath -Raw | ConvertFrom-Json
  if (-not $Url)    { $Url = $cfg.url }
  if (-not $AnonKey) { $AnonKey = $cfg.anon_key }
}
if (-not $Url -or -not $AnonKey) {
  Write-Host "Supabase proje bilgileri (Dashboard -> Project Settings -> API):"
  if (-not $Url)    { $Url = Read-Host "  Supabase URL" }
  if (-not $AnonKey) { $AnonKey = Read-Host "  anon key" }
}
if (-not $Url -or -not $AnonKey) {
  Write-Error "URL ve anon key zorunlu."
  exit 1
}
$ref = ($Url -replace "https://([^.]+)\.supabase\.co.*", '$1')
Write-Host "Proje ref: $ref"

# ─── 2) Admin token ─────────────────────────────────────────
if (-not $AdminToken) {
  $AdminToken = Read-Host "LICENSE_ADMIN_TOKEN (bos = otomatik uret)" -AsSecureString
  if ($AdminToken) {
    $AdminToken = [System.Net.NetworkCredential]::new("", $AdminToken).Password
  } else {
    $AdminToken = node -e "console.log(require('crypto').randomBytes(24).toString('hex'))"
  }
}

# ─── 3) Anahtar cifti ───────────────────────────────────────
$keyOutput = & node (Join-Path $root "scripts\gen-license-keys.mjs")
$privateKey = ""
$publicKey = ""
foreach ($line in $keyOutput) {
  if ($line -match "^(LICENSE_PRIVATE_KEY|LICENSE_PUBLIC_KEY) \(") { continue }
  if ($line -and -not $privateKey) { $privateKey = $line.Trim(); continue }
  if ($line) { $publicKey = $line.Trim() }
}
if (-not $privateKey -or -not $publicKey) {
  Write-Error "Anahtar cifti uretilemedi. Node 20+ gereklidir."
  exit 1
}

# ─── 4) Edge function deploy ────────────────────────────────
Push-Location (Join-Path $root "supabase")
try {
  Write-Host "Secret'lari yaziyorum (LICENSE_PRIVATE_KEY, LICENSE_ADMIN_TOKEN)..."
  & supabase secrets set "LICENSE_PRIVATE_KEY=$privateKey" "LICENSE_ADMIN_TOKEN=$AdminToken"
  if ($LASTEXITCODE -ne 0) { throw "Secret ayarlanamadi" }

  Write-Host "license fonksiyonu deploy ediliyor..."
  & supabase functions deploy license
  if ($LASTEXITCODE -ne 0) { throw "Fonksiyon deploy edilemedi" }
}
finally {
  Pop-Location
}

# ─── 5) Masaustu uygulama sabitleri ─────────────────────────
if (-not (Test-Path $licenseFile)) {
  Write-Warning "$licenseFile bulunamadi; kaynak sabitleri guncellenmedi."
  exit 0
}
$content = Get-Content $licenseFile -Raw

$escapedUrl = [System.Text.RegularExpressions.Regex]::Escape("https://YOUR-PROJECT-REF.supabase.co")
$content = $content -replace "const LICENSE_SERVER_URL: &str = `"$escapedUrl`";", "const LICENSE_SERVER_URL: &str = `"$Url`";"
$content = $content -replace "const LICENSE_ANON_KEY: &str = `"`";", "const LICENSE_ANON_KEY: &str = `"$AnonKey`";"
$content = $content -replace "pub const DEFAULT_PUBLIC_KEY_HEX: &str = `"`";", "pub const DEFAULT_PUBLIC_KEY_HEX: &str = `"$publicKey`";"

[System.IO.File]::WriteAllText($licenseFile, $content, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "license.rs guncellendi: LICENSE_SERVER_URL, LICENSE_ANON_KEY, DEFAULT_PUBLIC_KEY_HEX"

Write-Host ""
Write-Host "SONRAKI ADIMLAR:"
Write-Host "  1) Uygulamayi yeniden derleyin:"
Write-Host "     cd src-tauri; cargo tauri build"
Write-Host "  2) Ilk lisansi olusturmak icin:"
Write-Host "     curl -X POST `"$Url/functions/v1/license/generate`" -H `"Authorization: Bearer $AdminToken`" -H `"Content-Type: application/json`" -d `"{\`"business_name\`": \`"Kafe Adi\`"}`""
Write-Host "     (Cikti: JGJC-XXXXX-XXXXX-XXXXX lisans anahtari)"
Write-Host "  3) Lisans anahtarini makinelere girin: Ayarlar -> Lisans -> Aktif Et"
