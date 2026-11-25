// Standalone pure-Rust WhatsApp anti-delete bot
// Migrated from Node.js (wa-assistant) to pure Rust

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::{extract::State, response::Json, routing::get, serve, Router};
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};
use whatsapp_rust::download::{DownloadLimits, DownloadParams};
use whatsapp_rust::pair_code::PairCodeOptions;
use whatsapp_rust::prelude::*;
use whatsapp_rust::upload::UploadOptions;
use whatsapp_rust::wacore::download::MediaType;
use whatsapp_rust::wacore_binary::JidExt;

#[derive(Clone)]
struct AppState {
    message_cache: Arc<Mutex<MessageCache>>,
    anti_delete_enabled: Arc<RwLock<bool>>,
    target_jid: Option<Jid>,
    started_at: Arc<Instant>,
    /// Counters for the lightweight stats feature.
    stats: Arc<Mutex<Stats>>,
    /// Separate, much smaller cache for status@broadcast updates.
    status_cache: Arc<Mutex<HashMap<String, CachedMessage>>>,
}

#[derive(Default)]
struct Stats {
    captured: u64,
    recovered: u64,
    media_recovered: u64,
    revoked_ignored_self: u64,
    revoke_misses: u64,
    statuses_captured: u64,
    statuses_recovered: u64,
}

struct MessageCache {
    messages: HashMap<String, CachedMessage>,
    /// 0 = no cap (grows for the lifetime of the process).
    max_size: usize,
    /// None = entries never expire by age.
    ttl: Option<Duration>,
}

#[derive(Clone)]
struct CachedMessage {
    id: String,
    from: Jid,
    sender: Jid,
    sender_name: String,
    body: String,
    msg_type: String,
    message: Arc<wa::Message>,
    timestamp: u64,
    cached_at: u64,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    uptime_secs: u64,
    cached_messages: usize,
    anti_delete_enabled: bool,
    /// Resident set size in megabytes (physical RAM held by this process).
    rss_mb: f64,
    captured: u64,
    recovered: u64,
    media_recovered: u64,
    revoked_ignored_self: u64,
    revoke_misses: u64,
    statuses_captured: u64,
    statuses_recovered: u64,
}

/// Samples this process's resident memory without an external command.
fn rss_mb() -> f64 {
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), false);
    sys.process(pid)
        .map(|p| p.memory() as f64 / 1024.0 / 1024.0)
        .unwrap_or(0.0)
}

impl MessageCache {
    fn new(max_size: usize, ttl: Option<Duration>) -> Self {
        Self {
            messages: HashMap::new(),
            max_size,
            ttl,
        }
    }

    fn is_expired(ttl: &Option<Duration>, cached_at: u64, now: u64) -> bool {
        match ttl {
            Some(ttl) => now.saturating_sub(cached_at) >= ttl.as_secs(),
            None => false,
        }
    }

    fn insert(&mut self, msg: CachedMessage) {
        let now = current_timestamp();
        if self.ttl.is_some() {
            let ttl = &self.ttl;
            self.messages
                .retain(|_, v| !Self::is_expired(ttl, v.cached_at, now));
        }
        // Evict only when a finite cap is configured (max_size == 0 means unlimited).
        if self.max_size > 0 && self.messages.len() >= self.max_size {
            if let Some(oldest_key) = self.messages.keys().next().cloned() {
                self.messages.remove(&oldest_key);
            }
        }
        self.messages.insert(msg.id.clone(), msg);
    }

    fn get(&mut self, id: &str) -> Option<CachedMessage> {
        if self.ttl.is_some() {
            let now = current_timestamp();
            let ttl = &self.ttl;
            self.messages
                .retain(|_, v| !Self::is_expired(ttl, v.cached_at, now));
        }
        self.messages.remove(id)
    }

    fn len(&self) -> usize {
        self.messages.len()
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn format_time(timestamp: u64) -> String {
    use chrono::{DateTime, Utc};
    DateTime::<Utc>::from_timestamp(timestamp as i64, 0)
        .map(|dt| {
            dt.with_timezone(&chrono_tz::Asia::Jakarta)
                .format("%d/%m/%Y %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Draws the pairing payload as a scannable QR in the terminal.
fn print_qr(data: &str) {
    match qrcode::QrCode::new(data.as_bytes()) {
        Ok(code) => {
            let art = code
                .render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(true)
                .build();
            println!(
                "\n=== Scan di WhatsApp > Perangkat Tertaut > Tautkan Perangkat ===\n\n{}\n\n(QR berlaku ~20 detik; kalau kedaluwarsa, QR baru dicetak otomatis.)\n",
                art
            );
        }
        Err(e) => {
            println!("Gagal membuat QR ({e}). Payload mentah:\n{data}\n");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting pure-Rust WhatsApp anti-delete bot");

    // No hardcoded recipient. When TARGET_JID is unset, notifications go to this
    // bot's own account ("message yourself") instead of leaking to a stranger.
    let target_jid: Option<Jid> = match std::env::var("TARGET_JID") {
        Ok(v) if !v.trim().is_empty() => Some(
            v.trim()
                .parse::<Jid>()
                .map_err(|e| anyhow::anyhow!("TARGET_JID tidak valid ({v}): {e}"))?,
        ),
        _ => None,
    };

    let pair_phone = std::env::var("PAIR_PHONE").ok();
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()?;
    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "whatsapp.db".to_string());

    // Cache shape is configurable. `CACHE_MAX=0` disables the cap entirely and
    // `CACHE_TTL_DAYS=0` disables expiry, so nothing is ever evicted.
    // The default keeps RAM tiny on a 512 MB Render tier.
    let cache_max: usize = std::env::var("CACHE_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    let cache_ttl_days: u64 = std::env::var("CACHE_TTL_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);
    let cache_ttl =
        (cache_ttl_days > 0).then(|| Duration::from_secs(cache_ttl_days * 24 * 60 * 60));
    info!(
        "Cache: max {} pesan (0 = tanpa batas), TTL {} hari (0 = tanpa kedaluwarsa), in-memory",
        cache_max, cache_ttl_days
    );

    let state = AppState {
        message_cache: Arc::new(Mutex::new(MessageCache::new(cache_max, cache_ttl))),
        anti_delete_enabled: Arc::new(RwLock::new(true)),
        target_jid: target_jid.clone(),
        started_at: Arc::new(Instant::now()),
        stats: Arc::new(Mutex::new(Stats::default())),
        status_cache: Arc::new(Mutex::new(HashMap::new())),
    };

    // Health check server for Render
    let health_state = state.clone();
    let health_router = Router::new()
        .route("/health", get(health_check))
        .with_state(health_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("Health check server listening on 0.0.0.0:{}", port);
    tokio::spawn(async move {
        if let Err(e) = serve(listener, health_router).await {
            error!("Health server error: {}", e);
        }
    });

    let mut builder = Bot::builder()
        .with_backend(SqliteStore::open(&db_path).await?)
        .skip_history_sync();

    if let Some(phone) = pair_phone {
        builder = builder
            .with_pair_code(PairCodeOptions {
                phone_number: phone,
                ..Default::default()
            })
            .on_pair_code(|code, _timeout| async move {
                println!("Pairing code: {}", code);
            });
    } else {
        builder = builder.on_qr_code(|code, _timeout| async move {
            print_qr(code.as_str());
        });
    }

    // ---- handler 1: cache incoming messages ----
    let cache_a = state.message_cache.clone();
    let stats_a = state.stats.clone();
    let status_a = state.status_cache.clone();
    builder = builder.on_message(move |ctx: MessageContext| {
        let cache = cache_a.clone();
        let stats = stats_a.clone();
        let status = status_a.clone();
        let msg = ctx.message.clone();
        let info = ctx.info.clone();

        async move {
            if msg.protocol_message.is_set() {
                return;
            }

            let from = info.source.chat.clone();
            // Newsletters/channels are noisy and have no anti-delete value.
            if from.is_newsletter() {
                return;
            }

            // Status updates go into a separate, small cache (max 500).
            if from.is_status_broadcast() {
                let mut sc = status.lock().await;
                if sc.len() >= 500 {
                    if let Some(k) = sc.keys().next().cloned() {
                        sc.remove(&k);
                    }
                }
                sc.insert(
                    info.id.to_string(),
                    CachedMessage {
                        id: info.id.to_string(),
                        from: from.clone(),
                        sender: info.source.sender.clone(),
                        sender_name: info.push_name.to_string(),
                        body: extract_message_text(msg.as_ref()),
                        msg_type: classify_message_type(msg.as_ref()),
                        message: msg.clone(),
                        timestamp: info.timestamp.timestamp() as u64,
                        cached_at: current_timestamp(),
                    },
                );
                stats.lock().await.statuses_captured += 1;
                return;
            }

            let sender_jid = info.source.sender.clone();
            let sender_name = if !info.push_name.is_empty() {
                info.push_name.to_string()
            } else {
                sender_jid
                    .to_string()
                    .split('@')
                    .next()
                    .unwrap_or("Unknown")
                    .to_string()
            };

            let body = extract_message_text(msg.as_ref());
            let msg_type = classify_message_type(msg.as_ref());

            let cached = CachedMessage {
                id: info.id.to_string(),
                from: info.source.chat.clone(),
                sender: sender_jid,
                sender_name,
                body,
                msg_type: msg_type.clone(),
                message: msg.clone(),
                timestamp: info.timestamp.timestamp() as u64,
                cached_at: current_timestamp(),
            };

            cache.lock().await.insert(cached);
            stats.lock().await.captured += 1;
            debug!("Cached {} message {}", msg_type, info.id);
        }
    });

    // ---- handler 2: revoke -> anti-delete ----
    let cache_b = state.message_cache.clone();
    let anti_delete_b = state.anti_delete_enabled.clone();
    let target_b = state.target_jid.clone();
    let stats_b = state.stats.clone();
    let status_b = state.status_cache.clone();

    builder = builder.on_event_for(
        &[EventKind::Messages],
        move |event: Arc<Event>, client: Arc<Client>| {
            let cache = cache_b.clone();
            let anti_delete = anti_delete_b.clone();
            let target = target_b.clone();
            let stats = stats_b.clone();
            let status = status_b.clone();
            let client = client.clone();

            async move {
                let Event::Messages(msgs) = event.as_ref() else {
                    return;
                };

                for inbound in msgs {
                    let msg = &inbound.message;
                    if !msg.protocol_message.is_set() {
                        continue;
                    }
                    let Some(proto) = msg.protocol_message.as_option() else {
                        continue;
                    };
                    if proto.r#type != Some(wa::message::protocol_message::Type::REVOKE) {
                        continue;
                    }

                    let revoked_id = proto
                        .key
                        .as_option()
                        .and_then(|k| k.id.clone())
                        .unwrap_or_default();
                    if revoked_id.is_empty() {
                        continue;
                    }

                    // Status deletions live in the separate status cache.
                    let is_status_revoke = inbound.info.source.chat.is_status_broadcast();
                    let cached = if is_status_revoke {
                        let mut sc = status.lock().await;
                        let found = sc.remove(&revoked_id);
                        drop(sc);
                        match found {
                            Some(c) => c,
                            None => {
                                stats.lock().await.revoke_misses += 1;
                                continue;
                            }
                        }
                    } else {
                        let mut cache_guard = cache.lock().await;
                        let found = cache_guard.get(&revoked_id);
                        drop(cache_guard);
                        match found {
                            Some(c) => c,
                            None => {
                                stats.lock().await.revoke_misses += 1;
                                continue;
                            }
                        }
                    };

                    if !*anti_delete.read().await {
                        continue;
                    }

                    // Ignore revokes performed by this bot's own account.
                    let own_jid = client.pn();
                    if let Some(ref own) = own_jid {
                        if cached.sender == *own {
                            stats.lock().await.revoked_ignored_self += 1;
                            continue;
                        }
                    }

                    // Destination: TARGET_JID when set, otherwise this bot's own
                    // number ("message yourself"). Never a hardcoded stranger.
                    let destination = match target {
                        Some(ref jid) => jid.clone(),
                        None => match own_jid {
                            Some(jid) => jid,
                            None => {
                                warn!(
                                    "TARGET_JID belum diset dan nomor bot belum diketahui; notifikasi {} dilewati",
                                    revoked_id
                                );
                                continue;
                            }
                        },
                    };

                    info!(
                        "Anti-delete: message {} revoked by {} -> kirim ke {}",
                        revoked_id, cached.sender_name, destination
                    );

                    let deleted_at = format_time(current_timestamp());
                    let sent_at = format_time(cached.timestamp);
                    let location = if is_status_revoke {
                        "Status".to_string()
                    } else if cached.from.to_string().ends_with("@g.us") {
                        // Group subject when we can read it, otherwise a generic label.
                        match client.groups().fetch_metadata(&cached.from).await {
                            Ok(meta) => match meta.subject {
                                Some(s) if !s.trim().is_empty() => s,
                                _ => "Grup (tanpa nama)".to_string(),
                            },
                            Err(e) => {
                                debug!("Gagal ambil nama grup {}: {e}", cached.from);
                                "Grup".to_string()
                            }
                        }
                    } else {
                        "Chat Pribadi".to_string()
                    };

                    let title = if is_status_revoke {
                        "Status Dihapus!"
                    } else {
                        "Pesan Dihapus!"
                    };
                    let header = format!(
                        "🗑️ *{}*\n━━━━━━━━━━━━━━━\n👤 Dari: {}\n📍 Di: {}\n📤 Dikirim: {}\n🗑️ Dihapus: {}\n",
                        title, cached.sender_name, location, sent_at, deleted_at
                    );

                    let is_media = !matches!(cached.msg_type.as_str(), "text" | "unknown");
                    let _ = send_cached_message(&client, &destination, &cached, &header).await;

                    let mut s = stats.lock().await;
                    s.recovered += 1;
                    if is_media {
                        s.media_recovered += 1;
                    }
                    if is_status_revoke {
                        s.statuses_recovered += 1;
                    }
                }
            }
        },
    );

    // ---- handler 3: !rvo, !ping, !stats and !antidelete ----
    let anti_delete_c = state.anti_delete_enabled.clone();
    let started_c = state.started_at.clone();
    let target_c = state.target_jid.clone();
    builder = builder.on_message(move |ctx: MessageContext| {
        let anti_delete = anti_delete_c.clone();
        let started = started_c.clone();
        let target = target_c.clone();

        async move {
            let request_at = Instant::now();
            let body = extract_message_text(ctx.message.as_ref());
            let trimmed = body.trim();

            if trimmed.starts_with("!rvo") {
                let _ = handle_rvo(&ctx, target.clone()).await;
                return;
            }

            if trimmed.starts_with("!ping") {
                let ms = request_at.elapsed().as_millis();
                let _ = ctx.reply(format!("Pong! {}ms", ms)).await;
                return;
            }

            if trimmed.starts_with("!stats") {
                let up = started.elapsed().as_secs();
                let ms = request_at.elapsed().as_millis();
                let _ = ctx
                    .reply(format!(
                        "Uptime: {}s\nRAM: {:.1} MB\nPing: {}ms",
                        up,
                        rss_mb(),
                        ms
                    ))
                    .await;
                return;
            }

            if !trimmed.starts_with("!antidelete") {
                return;
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() != 2 {
                let status = if *anti_delete.read().await {
                    "enabled"
                } else {
                    "disabled"
                };
                let _ = ctx
                    .reply(format!(
                        "Anti-delete is currently {}. Use !antidelete on|off",
                        status
                    ))
                    .await;
                return;
            }

            let enabled = match parts[1].to_lowercase().as_str() {
                "on" | "enable" | "true" | "1" => true,
                "off" | "disable" | "false" | "0" => false,
                _ => {
                    let _ = ctx.reply("Usage: !antidelete on|off").await;
                    return;
                }
            };

            *anti_delete.write().await = enabled;
            let status = if enabled { "enabled" } else { "disabled" };
            let _ = ctx.reply(format!("Anti-delete {}", status)).await;
        }
    });

    let bot = builder.build().await?;
    info!("Bot built successfully, starting...");
    bot.run().await;

    Ok(())
}

async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    let cached_messages = state.message_cache.lock().await.len();
    let enabled = *state.anti_delete_enabled.read().await;
    let s = state.stats.lock().await;

    Json(HealthResponse {
        status: "ok".to_string(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        cached_messages,
        anti_delete_enabled: enabled,
        rss_mb: rss_mb(),
        captured: s.captured,
        recovered: s.recovered,
        media_recovered: s.media_recovered,
        revoked_ignored_self: s.revoked_ignored_self,
        revoke_misses: s.revoke_misses,
        statuses_captured: s.statuses_captured,
        statuses_recovered: s.statuses_recovered,
    })
}

fn extract_message_text(msg: &wa::Message) -> String {
    if msg.extended_text_message.is_set() {
        return msg.extended_text_message.text.clone().unwrap_or_default();
    }
    if let Some(conv) = &msg.conversation {
        return conv.clone();
    }
    if msg.image_message.is_set() {
        return msg.image_message.caption.clone().unwrap_or_default();
    }
    if msg.video_message.is_set() {
        return msg.video_message.caption.clone().unwrap_or_default();
    }
    if msg.document_message.is_set() {
        return msg.document_message.caption.clone().unwrap_or_default();
    }
    String::new()
}

fn classify_message_type(msg: &wa::Message) -> String {
    if msg.conversation.is_some() || msg.extended_text_message.is_set() {
        return "text".to_string();
    }
    if msg.image_message.is_set() {
        return "image".to_string();
    }
    if msg.video_message.is_set() {
        return "video".to_string();
    }
    if msg.audio_message.is_set() {
        return "audio".to_string();
    }
    if msg.sticker_message.is_set() {
        return "sticker".to_string();
    }
    if msg.document_message.is_set() {
        return "document".to_string();
    }
    "unknown".to_string()
}

/// !rvo — reveal a quoted view-once media by downloading it with its original
/// CDN metadata and re-uploading it under a fresh key we own.
async fn handle_rvo(ctx: &MessageContext, target: Option<Jid>) -> Result<()> {
    let quoted = ctx
        .message
        .extended_text_message
        .as_option()
        .and_then(|e| e.context_info.as_option())
        .and_then(|c| c.quoted_message.as_option());

    let Some(q) = quoted else {
        let _ = ctx
            .reply("❌ Reply pesan view once dulu, lalu ketik *!rvo*")
            .await;
        return Ok(());
    };

    // Unwrap view-once wrappers (V2, V2Extension, legacy) and ephemeral.
    let mut inner = q;
    for _ in 0..4 {
        let next = inner
            .view_once_message_v2
            .as_option()
            .and_then(|w| w.message.as_option())
            .or_else(|| {
                inner
                    .view_once_message
                    .as_option()
                    .and_then(|w| w.message.as_option())
            })
            .or_else(|| {
                inner
                    .view_once_message_v2_extension
                    .as_option()
                    .and_then(|w| w.message.as_option())
            })
            .or_else(|| {
                inner
                    .ephemeral_message
                    .as_option()
                    .and_then(|w| w.message.as_option())
            });
        match next {
            Some(n) => inner = n,
            None => break,
        }
    }

    let view_once = inner
        .image_message
        .as_option()
        .filter(|m| m.view_once == Some(true))
        .map(|m| {
            (
                "image",
                m.mimetype.as_deref(),
                m.file_sha256.as_deref(),
                m.file_enc_sha256.as_deref(),
                m.file_length,
                m.media_key.as_deref(),
                m.direct_path.as_deref(),
                m.url.as_deref(),
            )
        })
        .or_else(|| {
            inner
                .video_message
                .as_option()
                .filter(|m| m.view_once == Some(true))
                .map(|m| {
                    (
                        "video",
                        m.mimetype.as_deref(),
                        m.file_sha256.as_deref(),
                        m.file_enc_sha256.as_deref(),
                        m.file_length,
                        m.media_key.as_deref(),
                        m.direct_path.as_deref(),
                        m.url.as_deref(),
                    )
                })
        })
        .or_else(|| {
            inner
                .audio_message
                .as_option()
                .filter(|m| m.view_once == Some(true))
                .map(|m| {
                    (
                        "audio",
                        m.mimetype.as_deref(),
                        m.file_sha256.as_deref(),
                        m.file_enc_sha256.as_deref(),
                        m.file_length,
                        m.media_key.as_deref(),
                        m.direct_path.as_deref(),
                        m.url.as_deref(),
                    )
                })
        });

    let Some((kind, mime, sha, enc_sha, len, key, direct, url)) = view_once else {
        let _ = ctx
            .reply("❌ Pesan yang direply bukan view once atau tipe tidak didukung.")
            .await;
        return Ok(());
    };
    let (Some(sha), Some(enc_sha), Some(len), Some(key)) = (sha, enc_sha, len, key) else {
        let _ = ctx
            .reply("❌ Metadata media view once tidak lengkap.")
            .await;
        return Ok(());
    };
    if key.len() != 32 || sha.len() != 32 || enc_sha.len() != 32 {
        let _ = ctx.reply("❌ Kunci media tidak valid.").await;
        return Ok(());
    }

    let path = direct
        .or(url)
        .ok_or_else(|| anyhow::anyhow!("tidak ada directPath/url pada media"))?;

    let array: &[u8; 32] = key.try_into()?;
    let media_type = match kind {
        "image" => MediaType::Image,
        "video" => MediaType::Video,
        _ => MediaType::Audio,
    };
    let params = DownloadParams::encrypted(path, key, sha, enc_sha, len, media_type)?;

    let _ = ctx
        .reply("⏳ Mengambil media view once... hasil dikirim ke chat pribadi.")
        .await;
    let bytes = ctx
        .client
        .download(&params, DownloadLimits::default())
        .await?;

    let uploaded = ctx
        .client
        .upload(bytes, media_type, UploadOptions::default())
        .await?;

    let mime = mime.unwrap_or(match kind {
        "image" => "image/jpeg",
        "video" => "video/mp4",
        _ => "audio/mp4",
    });

    let mut out = wa::Message::default();
    match kind {
        "image" => {
            let mut m = wa::message::ImageMessage::default();
            m.mimetype = Some(mime.to_string());
            m.file_length = Some(uploaded.file_length);
            m.file_sha256 = Some(uploaded.file_sha256.to_vec());
            m.file_enc_sha256 = Some(uploaded.file_enc_sha256.to_vec());
            m.media_key = Some(uploaded.media_key.to_vec());
            m.media_key_timestamp = Some(uploaded.media_key_timestamp);
            m.direct_path = Some(uploaded.direct_path.clone());
            m.url = Some(uploaded.url.clone());
            out.image_message = MessageField::some(m);
        }
        "video" => {
            let mut m = wa::message::VideoMessage::default();
            m.mimetype = Some(mime.to_string());
            m.file_length = Some(uploaded.file_length);
            m.file_sha256 = Some(uploaded.file_sha256.to_vec());
            m.file_enc_sha256 = Some(uploaded.file_enc_sha256.to_vec());
            m.media_key = Some(uploaded.media_key.to_vec());
            m.media_key_timestamp = Some(uploaded.media_key_timestamp);
            m.direct_path = Some(uploaded.direct_path.clone());
            m.url = Some(uploaded.url.clone());
            out.video_message = MessageField::some(m);
        }
        _ => {
            let mut m = wa::message::AudioMessage::default();
            m.mimetype = Some(mime.to_string());
            m.file_length = Some(uploaded.file_length);
            m.file_sha256 = Some(uploaded.file_sha256.to_vec());
            m.file_enc_sha256 = Some(uploaded.file_enc_sha256.to_vec());
            m.media_key = Some(uploaded.media_key.to_vec());
            m.media_key_timestamp = Some(uploaded.media_key_timestamp);
            m.direct_path = Some(uploaded.direct_path.clone());
            m.url = Some(uploaded.url.clone());
            out.audio_message = MessageField::some(m);
        }
    }
    let _array = array;
    // Hasil !rvo selalu ke chat pribadi (TARGET_JID / chat sendiri), bukan ke grup.
    let destination = match target {
        Some(jid) => jid,
        None => match ctx.client.pn() {
            Some(jid) => jid,
            None => {
                warn!("RVO: nomor bot tidak diketahui, media tidak dikirim");
                return Ok(());
            }
        },
    };
    ctx.client.send_message(destination, out).await?;
    Ok(())
}

async fn send_cached_message(
    client: &Arc<Client>,
    target: &Jid,
    cached: &CachedMessage,
    header: &str,
) -> Result<()> {
    let proto = cached.message.as_ref();
    let is_media = matches!(
        cached.msg_type.as_str(),
        "image" | "video" | "audio" | "sticker" | "document"
    );

    if is_media {
        // Header bubble, then the relayed media (forward keeps the CDN blob).
        let mut notice = wa::Message::default();
        notice.conversation = Some(header.to_string());
        let _ = client.send_message(target, notice).await?;

        if let Err(e) = client.forward_message(target, proto).await {
            warn!("Gagal forward media {}: {e}", cached.msg_type);
            let mut fallback = wa::Message::default();
            fallback.conversation = Some(format!(
                "⚠️ Media {} gagal diteruskan.",
                cached.msg_type
            ));
            let _ = client.send_message(target, fallback).await?;
        }
    } else {
        // Single bubble: header + original text. No separate header bubble.
        let mut out = wa::Message::default();
        out.conversation = Some(if cached.msg_type == "text" {
            format!("{}💬 {}", header, cached.body)
        } else {
            format!("{}💬 {}", header, cached.msg_type)
        });
        let _ = client.send_message(target, out).await?;
    }

    Ok(())
}
