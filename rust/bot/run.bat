@echo off
setlocal
title WA Bot
mode con: cols=100 lines=45 >nul 2>&1
chcp 65001 >nul
cd /d "%~dp0"

if not defined PORT set PORT=8787
if not defined RUST_LOG set RUST_LOG=warn
if not "%~1"=="" set PAIR_PHONE=%~1

rem Tujuan notifikasi anti-delete.
rem Kalau tidak diset, bot mengirim ke nomor akunnya sendiri ("pesan ke diri sendiri").
rem Isi baris di bawah untuk mengirim ke nomor lain:
rem set TARGET_JID=6281234567890@s.whatsapp.net

rem Cache: 0 = tanpa batas jumlah pesan, 0 hari = tanpa kedaluwarsa.
rem Default kalau tidak diset: CACHE_MAX=500, CACHE_TTL_DAYS=7
set CACHE_MAX=0
set CACHE_TTL_DAYS=0

echo ==================================================
echo   WA Bot - pure Rust (anti-delete)
echo --------------------------------------------------
echo   PORT       : %PORT%
if defined PAIR_PHONE (
  echo   PAIR_PHONE : %PAIR_PHONE%  ^(pairing code^)
) else (
  echo   PAIR_PHONE : -  ^(QR code^)
)
echo ==================================================
echo.

target\debug\wa-bot.exe

echo.
echo ==== Bot berhenti (exit code %ERRORLEVEL%) ====
echo Tekan tombol apa saja untuk menutup jendela ini.
pause >nul
