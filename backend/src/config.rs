use anyhow::Result;

#[derive(Clone, Debug)]
pub struct Config {
    pub api_addr: String,
    pub otlp_addr: String,
    pub clickhouse_url: String,
    pub clickhouse_database: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
    pub redis_url: Option<String>,
    pub batch_max_rows: usize,
    pub batch_flush_ms: u64,
    pub public_base_url: String,
    /// Token global del bot de Telegram. Si se define, los targets `tg://<chat_id>`
    /// usan este bot. Los targets `tg://<chat_id>@<token>` siempre pueden traer
    /// su propio token y no necesitan que esté configurado a nivel global.
    pub telegram_bot_token: Option<String>,
    /// Base de la API de Telegram. Configurable solo para pruebas — en producción
    /// se usa el valor por defecto.
    pub telegram_api_base: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            api_addr: env_or("FARO_API_ADDR", "0.0.0.0:8080"),
            otlp_addr: env_or("FARO_BIND_ADDR", "0.0.0.0:4318"),
            clickhouse_url: env_or("CLICKHOUSE_URL", "http://localhost:8123"),
            clickhouse_database: env_or("CLICKHOUSE_DATABASE", "faro"),
            clickhouse_user: env_or("CLICKHOUSE_USER", "faro"),
            clickhouse_password: env_or("CLICKHOUSE_PASSWORD", "faro"),
            redis_url: std::env::var("REDIS_URL").ok(),
            batch_max_rows: env_or("FARO_BATCH_MAX_ROWS", "5000")
                .parse()
                .unwrap_or(5000),
            batch_flush_ms: env_or("FARO_BATCH_FLUSH_MS", "750")
                .parse()
                .unwrap_or(750),
            public_base_url: env_or("FARO_PUBLIC_BASE_URL", "http://localhost:8080"),
            telegram_bot_token: std::env::var("TELEGRAM_BOT_TOKEN")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            telegram_api_base: env_or("TELEGRAM_API_BASE", "https://api.telegram.org"),
        })
    }
}

fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}
