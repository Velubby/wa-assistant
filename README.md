# WhatsApp Anti-Delete Bot

Bot WhatsApp untuk mendeteksi dan menyelamatkan pesan/media yang dihapus ("delete for everyone"), lalu meneruskannya ke tujuan yang dikonfigurasi.

Bot ini berjalan sebagai satu binary tanpa runtime Node.js.

---

## Fitur

- Menangkap pesan teks dan media yang dihapus via relay CDN (tanpa download/re-upload).
- Anti-delete untuk status WhatsApp (`status@broadcast`) dengan judul `🗑️ *Status Dihapus!*`.
- Nama grup asli ditampilkan di notifikasi.
- `!antidelete on|off` — nyalakan/matikan fitur saat runtime (default: on).
- `!stats` — uptime, RAM, dan ping.
- `!ping` — balas `Pong!` + latency.
- `!rvo` — buka media *view once* dengan reply pesan (image/video/audio).
- `GET /health` — uptime, RAM, dan counter untuk health check.
- Login via pairing code atau QR; sesi tersimpan di SQLite (`DB_PATH`).

Command dari bot lama (`!yt`, `!ig`, `!sticker`, `!ocr`, `!jarvis`, dll.) **belum tersedia**.

---

## Struktur

```
.
├── Dockerfile.rust          # image Docker
├── docker-compose.yml       # jalankan lokal/Docker dengan volume /data
├── render.yaml              # blueprint Render Web Service
└── rust/bot/
    ├── Cargo.toml
    ├── Cargo.lock
    ├── src/main.rs          # seluruh logika bot
    ├── README.md            # ringkasan teknis
    ├── USAGE.md             # panduan lengkap & troubleshooting
    └── run.bat              # jalankan langsung di Windows
```

---

## Menjalankan

### Windows (langsung)

```bat
cd rust\bot
cargo build --release
.\run.bat
```

`run.bat` memakai port `8787` dan `whatsapp.db` di folder tersebut. Untuk pairing code:

```bat
.\run.bat 6281234567890
```

### Docker

```bash
docker compose up -d --build
docker compose logs -f
```

Sesi WhatsApp disimpan di named volume `wa-data` yang di-mount di `/data`.

### Render

`render.yaml` sudah mendefinisikan Web Service Docker dari `Dockerfile.rust` dengan disk 1 GB di `/data`. Set `PAIR_PHONE` dan `TARGET_JID` sebagai secret env var.

Update deployment:

```bash
./update.sh     # Linux/macOS
update.bat      # Windows
```

---

## Konfigurasi

| Variable | Default | Arti |
|---|---|---|
| `PAIR_PHONE` | kosong → QR | Nomor untuk login pairing code (format internasional, angka saja) |
| `TARGET_JID` | akun bot sendiri | Tujuan notifikasi, mis. `6281234567890@s.whatsapp.net` |
| `PORT` | `8080` | Port HTTP health check |
| `DB_PATH` | `whatsapp.db` | File sesi SQLite |
| `CACHE_MAX` | `500` | Maksimal pesan di cache; `0` = tanpa batas |
| `CACHE_TTL_DAYS` | `7` | Umur entri cache (hari); `0` = tanpa kedaluwarsa |
| `RUST_LOG` | kosong | `info` / `debug` untuk melihat log koneksi |

Kalau `TARGET_JID` tidak diset, notifikasi masuk ke "pesan ke diri sendiri" akun yang dipairing. Tidak ada nomor hardcoded di kode.

---

## Health check

```bash
curl http://127.0.0.1:8080/health
```

```json
{
  "status": "ok",
  "uptime_secs": 38,
  "cached_messages": 50,
  "anti_delete_enabled": true,
  "rss_mb": 33.48,
  "captured": 50,
  "recovered": 0,
  "media_recovered": 0,
  "revoked_ignored_self": 0,
  "revoke_misses": 0
}
```

---

## Batasan

- Cache **in-memory**: hilang saat bot restart, jadi pesan sebelum restart tidak tertangkap.
- Grup ramai bisa mebuang pesan lama sebelum dihapus — naikkan `CACHE_MAX` bila perlu.
- Media diteruskan lewat `forward_message`; kalau gagal, bot mengirim pesan peringatan.
- Newsletter/channel diabaikan karena tidak ada nilai anti-delete.

Detail lengkap ada di [`rust/bot/USAGE.md`](rust/bot/USAGE.md).

---

## ⚠️ Disclaimer

Proyek ini hanya untuk penggunaan **edukatif, arsip, administratif, dan/atau dengan izin yang sah**. Penulis tidak bertanggung jawab atas penyalahgunaan, pelanggaran privasi, maupun konsekuensi hukum yang timbul. Gunakan hanya di lingkungan yang kamu miliki atau punya izin eksplisit, dan patuhi hukum serta aturan platform yang berlaku.
