# Oyun Kafe Yönetim Paneli

Rust + Tauri ile geliştirilmiş, oyun kafeler için saatlik ücret hesaplayan masaüstü yönetim paneli.

## Ozellikler

- **Gercek Zamanli Dashboard** — Aktif oturumlari anlik takip et
- **Istasyon Yonetimi** — PC ekle, sil, durumlarini goruntule
- **Cift Ucret Sistemi** — Nakit 4.20TL/dk | Kart 5.00TL/dk
- **Icecek Yonetimi** — Su 30TL, Soda 30TL, Kutu Icecek 80TL + yeni icecek ekleme
- **Canli Ucret Takibi** — Her saniye guncellenen anlik ucret gosterimi
- **Icecek Toplam Ucreti** — Icecek siparisleri otomatik olarak oturum toplamina eklenir
- **Oturum Gecmisi** — Tum oturumlari kaydet ve raporla
- **Odeme Yontemleri** — Nakit ve kart takibi
- **Baslangic Saati Duzenleme** — Aktif oturumun baslangic saati manuel degistirilebilir
- **Manuel Bitis Saati** — Oturumu sonlandırırken otomatik veya manuel bitis saati secenegi
- **SQLite Veritabani** — Veriler kalici olarak saklanir, uygulama kapaninca silinmez
- **Otomatik Yedekleme** — Her baslatildiginda ve istege bagli yedekleme
- **Tam Ekran / Kiosk Modu** — F11 veya buton ile tam ekran (uygulama penceresi)
- **Gelir Grafikleri** — Gunluk / haftalik / aylik gelir grafikleri (Chart.js)
- **Istatistikler** — En cok kullanilan istasyon, en cok siparis edilen icecek, sure trendi
- **Disa Aktarma** — Gecmis verilerini CSV veya JSON olarak disa aktar
- **Tauri Updater** — Otomatik guncelleme altyapisi

## Ucretlendirme

| Odeme Yontemi | Dakika Basi Ucret |
|---------------|-------------------|
| Nakit | **4.20 TL/dk** |
| Kart | **5.00 TL/dk** |

**Ornek:** 30 dk oynayan musteri
- Nakit: 30 x 4.20 = **126.00 TL**
- Kart: 30 x 5.00 = **150.00 TL**

## Icecek Menusu (Varsayilan)

| Icecek | Fiyat |
|--------|-------|
| Su | 30 TL |
| Soda | 30 TL |
| Kutu Icecek | 80 TL |

Yeni icecek ekleyebilir, mevcutlari silebilirsiniz.

## Kurulum

### Gereksinimler

- [Rust](https://rustup.rs/) (1.70+)
- [Node.js](https://nodejs.org/) (18+)
- [Tauri CLI](https://tauri.app/v1/guides/getting-started/prerequisites)

### Windows Kurulumu

```powershell
# 1. Rust kur (PowerShell Yonetici olarak)
winget install Rustlang.Rustup

# 2. PowerShell'i kapatip yeniden ac
# 3. Tauri CLI kur
cargo install tauri-cli

# 4. Projeyi indir ve cikar
# 5. Calistir
cd oyun-kafe-yonetim/src-tauri
cargo tauri dev
```

### EXE Olarak Derle

```bash
cd src-tauri
cargo tauri build
```

Cikti: `src-tauri/target/release/bundle/msi/*.msi`

## Veri Konumu

Veriler `%LOCALAPPDATA%/oyun-kafe-yonetim/` dizininde saklanir:
- `database.sqlite` — Ana veritabani
- `backups/` — Otomatik yedekler (son 30 yedek saklanir)
- `exports/` — Disa aktarilan CSV/JSON dosyalari

## Kullanim

### Oturum Baslatma
1. **Istasyonlar** sekmesinden bos bir PC sec
2. Musteri adini gir
3. Tahmini odeme yontemini sec (sonradan degistirilebilir)
4. "Baslat"

### Oturum Sonlandirma
1. **Genel Bakis** veya **Istasyonlar** sekmesinden aktif oturumu bul
2. "Sonlandir" butonuna tikla
3. Gercek odeme yontemini sec (Nakit/Kart)
4. Otomatik veya manuel bitis saati sec
5. Sistem otomatik ucret hesaplar (icecek dahil)

### Icecek Siparisi
1. **Icecekler** sekmesine git
2. Siparis vermek istedigin icecege tikla
3. Aktif oturumu ve adedi sec
4. "Siparis Ver"
5. Siparis tutari otomatik olarak oturum toplamina eklenir

### Gelir Grafikleri
1. **Istatistikler** sekmesine git
2. Gunluk/haftalik/aylik secenegini sec
3. Istasyon ve icecek grafiklerini incele

### Tam Ekran Modu
- F11 tusuna bas veya ust panelden "Tam Ekran" butonuna tikla

### Yedekleme
- "Yedekle" butonuna tikla (uygulama baslarken de otomatik yedekler)

### Disa Aktarma
1. **Gecmis** sekmesine git
2. CSV veya JSON butonuna tikla
3. Dosya `%LOCALAPPDATA%/oyun-kafe-yonetim/exports/` icine kaydedilir

## Proje Yapisi

```
oyun-kafe-yonetim/
├── src/
│   └── index.html              # Frontend (HTML/CSS/JS + Chart.js)
├── src-tauri/
│   ├── Cargo.toml              # Rust bagimliliklari
│   ├── tauri.conf.json         # Tauri yapilandirmasi
│   ├── build.rs                # Derleme scripti
│   ├── icons/                  # Uygulama ikonlari
│   └── src/
│       └── main.rs             # Rust backend (SQLite + Tauri commands)
└── README.md
```

## Teknolojiler

- **Rust** — Backend ve state yonetimi
- **Tauri v1.5** — Masaustu uygulama cercevesi
- **SQLite (rusqlite)** — Kalici veri depolama
- **Tailwind CSS** — UI stilizasyonu
- **Chart.js** — Grafik ve istatistikler

## Lisans

MIT
