# Cara pakai `wa-bot`

Bot ini **hanya** berisi fitur anti-delete. Semua command lama dari bot Node
(`!yt`, `!ig`, `!sticker`, `!ocr`, `!jarvis`, dst.) belum diport.

---

## 1. Fitur yang tersedia

| Fitur | Status |
|---|---|
| Menangkap pesan masuk ke cache | ✅ |
| Anti-delete: teks + media via CDN relay (tanpa re-upload) | ✅ |
| Anti-delete **status WhatsApp** (`status@broadcast`) | ✅ |
| Nama grup asli di notifikasi | ✅ |
| `!antidelete on` / `off` | ✅ |
| `!stats` — uptime, RAM, dan ping | ✅ |
| `!ping` — balas Pong + latency | ✅ |
| `!rvo` — buka media view once (download + re-upload), hasil ke chat pribadi | ✅ image, video, audio |
| Endpoint `GET /health`: uptime, RAM (RSS), dan counter | ✅ |
| Login pairing code / QR, sesi tersimpan di SQLite | ✅ |
| Cache tanpa batas (`CACHE_MAX=0`, `CACHE_TTL_DAYS=0`) | ✅ opsional |
| Skip newsletter/channel (tanpa nilai anti-delete) | ✅ |
| Command lain (`!help`, `!ping`, downloader, stiker, OCR, Jarvis) | ❌ |

---

## 2. Menjalankan

### Lokal (Windows)

```bat
cd C:\ivan-dev\wa-assistant\rust\bot
.\run.bat
```

`run.bat` akan:

- memakai port `8787` (bisa diubah lewat `PORT`),
- memakai `whatsapp.db` di folder itu sebagai sesi,
- mencetak QR **hanya kalau belum pernah pairing**.

Pairing code (tanpa scan QR):

```bat
.\run.bat 6281234567890
```

### Konfigurasi lewat environment

| Variable | Default | Arti |
|---|---|---|
| `PORT` | `8080` (`run.bat`: `8787`) | Port HTTP health check |
| `TARGET_JID` | **nomor akun bot sendiri** | Tujuan notifikasi, format `628xx@s.whatsapp.net` |
| `PAIR_PHONE` | kosong → QR | Nomor untuk login pairing code |
| `DB_PATH` | `whatsapp.db` | Lokasi file sesi SQLite |
| `CACHE_MAX` | `500` | Jumlah pesan maksimum di cache (lihat §4) |
| `CACHE_TTL_DAYS` | `7` | Umur maksimum entri cache, dalam hari |
| `RUST_LOG` | kosong (`run.bat`: `warn`) | `info` untuk melihat log koneksi |

Contoh mengirim notifikasi ke nomor lain — edit `run.bat`, aktifkan baris:

```bat
set TARGET_JID=6281234567890@s.whatsapp.net
```

> Kalau `TARGET_JID` tidak diset, notifikasi masuk ke **"pesan ke diri sendiri"**
> akun yang dipairing. Tidak ada nomor hardcoded di kode.

---

## 3. Command

Bot memproses **dua** command:

| Perintah | Efek |
|---|---|
| `!stats` | Balas uptime, RAM, dan ping |
| `!ping` | Balas `Pong! <ms>` |
| `!antidelete` | Balas status sekarang: `Anti-delete is currently enabled.` |
| `!antidelete on` | Nyalakan anti-delete |
| `!antidelete off` | Matikan anti-delete |
| `!antidelete 1` / `true` / `enable` | Sama dengan `on` |
| `!antidelete 0` / `false` / `disable` | Sama dengan `off` |
| `!rvo` (reply pesan view once) | Download media view once, upload ulang dengan kunci bot, kirim ke chat pribadi |

Contoh:

```
!antidelete        → Anti-delete is currently enabled. Use !antidelete on|off
!antidelete off    → Anti-delete disabled
!stats             → Uptime: 38s
                   RAM: 32.3 MB
                   Ping: 1ms
!ping              → Pong! 1ms
```

### Catatan `!rvo`

- Harus **reply** pesan view once (bukan kirim biasa).
- Mendukung wrapper `viewOnceMessageV2`, `viewOnceMessageV2Extension`, legacy
  `viewOnceMessage`, dan `ephemeralMessage`, untuk image/video/audio.
- Media diundang pakai kunci asli lalu di-upload ulang dengan kunci bot —
  jadi butuh bandwidth dan sedikit RAM (proses, bukan disimpan).
- Hasil media dikirim ke `TARGET_JID` (atau chat sendiri kalau belum diset),
  bukan ke grup tempat command diketik.
- Kalau metadata tidak lengkap, bot membalas pesan error.

---

## 4. Cara kerja anti-delete

1. Setiap pesan masuk disimpan ke cache **in-memory**:
   - maksimal **`CACHE_MAX` pesan** (default 500; paling lama dibuang lebih dulu / FIFO),
   - kedaluwarsa **`CACHE_TTL_DAYS` hari** (default 7).
2. Saat ada event *revoke* ("hapus untuk semua orang"), bot mencari pesan aslinya
   di cache lalu mengirim salinannya ke tujuan.
3. Revoke yang dilakukan oleh akun bot sendiri **diabaikan** (tidak memicu notifikasi).

Format notifikasi yang dikirim:

```
🗑️ *Pesan Dihapus!*
━━━━━━━━━━━━━━━
👤 Dari: <nama pengirim>
📍 Di: Grup | Chat Pribadi
📤 Dikirim: 25/11/2026 18:30:00
🗑️ Dihapus: 25/11/2026 18:31:12

💬 <isi pesan>
```

Semua waktu memakai zona **Asia/Jakarta (WIB)**.

### Batasan penting

- **Cache hilang saat bot restart.** Pesan yang masuk sebelum restart lalu
  dihapus setelah restart tidak akan tertangkap.
- **Jumlah pesan terbatas (`CACHE_MAX`).** Di grup ramai, pesan lama bisa
  terbuang sebelum sempat dihapus. Naikkan `CACHE_MAX` kalau perlu (lihat §7).
- **Nama grup** diambil dari metadata grup; kalau gagal dibaca, tampil `Grup`.
- **Media diteruskan lewat `forward_message`.** Library me-relay blob CDN asli,
  jadi tidak ada download/re-upload (raman RAM & bandwidth). Kalau forward gagal,
  bot mengirim pesan peringatan sebagai gantinya.
- **Status WhatsApp tidak dibuang.** Status masuk ke cache terpisah (maks. 500)
  dan notifikasinya memakai judul `🗑️ *Status Dihapus!*` + `📍 Di: Status`.
- **Newsletter/channel diabaikan** supaya tidak memenuhi cache.

---

## 5. Health check

```
GET http://127.0.0.1:8787/health
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

| Field | Arti |
|---|---|
| `uptime_secs` | Detik sejak bot start (sekarang sudah nyata, bukan `0`) |
| `rss_mb` | RAM fisik yang dipakai proses (resident set size) |
| `cached_messages` | Jumlah pesan di cache saat ini |
| `captured` | Total pesan yang pernah masuk & disimpan |
| `recovered` | Total pesan dihapus yang berhasil diteruskan |
| `media_recovered` | Bagian dari `recovered` yang berupa media |
| `revoked_ignored_self` | Revoke oleh akun bot sendiri yang diabaikan |
| `revoke_misses` | Revoke yang tidak ketemu di cache (misal pesan sebelum start / sudah terbuang) |

---

## 6. Troubleshooting

| Gejala | Penyebab / solusi |
|---|---|
| `Only one usage of each socket address` | Port dipakai proses lain — set `PORT` lain, mis. `set PORT=8788` |
| Notifikasi tidak muncul | Jendela/terminal bot tertutup, atau pesan dikirim sebelum bot start |
| Notifikasi masuk ke nomor sendiri | Normal kalau `TARGET_JID` belum diset |
| QR tidak terbaca | Perbesar font console (Properties → Font → Consolas 16) atau pakai pairing code |
| Diminta pairing terus | `whatsapp.db` hilang/terhapus — pair ulang |
| Media gagal tampil | Batasan yang sudah diketahui (lihat §4) |

Untuk melihat log koneksi:

```bat
set RUST_LOG=info
.\run.bat
```

---

## 7. Menyetel ukuran cache

Default `CACHE_MAX=500` dipilih supaya RAM tetap kecil di Render 512 MB.
Satu entri hanya menyimpan **protobuf pesan** (teks, caption, id media) — bukan
berkas medianya — jadi kira-kira **1–3 KB per entri**:

| `CACHE_MAX` | Perkiraan RAM | Cocok untuk |
|---|---|---|
| 500 (default) | ~1–2 MB | chat pribadi / grup kecil |
| 2.000 | ~3–6 MB | grup aktif |
| 10.000 | ~15–30 MB | grup sangat ramai |

Naikkan kalau grup kamu ramai dan pesan sering terhapus lebih lambat dari laju
pesan masuk:

```bat
set CACHE_MAX=2000
set CACHE_TTL_DAYS=7
.\run.bat
```

Angka ini hanya perkiraan; pantau lewat `cached_messages` di `/health`.
