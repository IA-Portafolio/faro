//! farocli — tail/query/monitor management contra una instancia de Faro.
//!
//! Reusa la API del dashboard tal cual está: auth por cookie de sesión obtenida
//! con `farocli login`, persistida en `~/.config/farocli/config.json`. Las
//! subcomandos son fachadas sobre los endpoints que ya consume el frontend.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const DEFAULT_ENDPOINT: &str = "http://localhost:8080";
const CONFIG_FILE: &str = "config.json";

#[derive(Parser)]
#[command(name = "farocli", version, about = "Tail/query/monitor management para Faro")]
struct Cli {
    /// URL base de la API. Sobreescribe el endpoint guardado.
    #[arg(long, env = "FARO_ENDPOINT", global = true)]
    endpoint: Option<String>,

    /// Slug del proyecto. Aplica a comandos que filtran por proyecto.
    #[arg(short = 'p', long, env = "FARO_PROJECT", global = true)]
    project: Option<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Login interactivo. Guarda la cookie de sesión en config.
    Login {
        #[arg(long)]
        email: Option<String>,
    },
    /// Borra la sesión local. No revoca la del servidor — usa el dashboard para eso.
    Logout,
    /// Lista o sigue (`--follow`) logs.
    Logs(LogsArgs),
    /// Servicios visibles con conteos de logs y errores.
    Services,
    /// Issues de errores agrupados por fingerprint.
    Errors {
        #[arg(long)]
        status: Option<String>,
    },
    /// Operaciones sobre monitores HTTP.
    #[command(subcommand)]
    Monitor(MonitorCmd),
}

#[derive(Args)]
struct LogsArgs {
    #[arg(short = 's', long)]
    service: Option<String>,
    /// Severidad mínima: DEBUG | INFO | WARN | ERROR | FATAL.
    #[arg(long)]
    severity: Option<String>,
    /// Substring que aparece en el body.
    #[arg(short = 'q', long)]
    query: Option<String>,
    /// Ventana de tiempo: 5m, 30m, 1h, 6h, 24h, 7d…
    #[arg(long, default_value = "1h")]
    last: String,
    /// Streaming en vivo vía SSE — corta con Ctrl-C.
    #[arg(short = 'f', long)]
    follow: bool,
    /// Imprime cada log como una línea JSON (jq-friendly).
    #[arg(long)]
    json: bool,
    /// Tope de logs a traer en modo no-follow.
    #[arg(long, default_value_t = 200)]
    limit: u32,
}

#[derive(Subcommand)]
enum MonitorCmd {
    /// Lista monitores del proyecto.
    List,
    /// Crea un monitor HTTP.
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        url: String,
        #[arg(long, default_value = "GET")]
        method: String,
        #[arg(long, default_value_t = 60)]
        interval: u32,
        #[arg(long, default_value_t = 30)]
        timeout: u32,
    },
}

// ---------- Config persistido ----------

#[derive(Serialize, Deserialize, Default)]
struct StoredConfig {
    endpoint: Option<String>,
    /// Valor crudo de la cookie `faro_session`.
    session: Option<String>,
}

fn config_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().ok_or_else(|| anyhow!("no encontré el directorio de config del SO"))?;
    Ok(dir.join("farocli").join(CONFIG_FILE))
}

fn load_config() -> StoredConfig {
    let Ok(path) = config_path() else { return StoredConfig::default() };
    let Ok(bytes) = std::fs::read(&path) else { return StoredConfig::default() };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_config(cfg: &StoredConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("creando directorio de config")?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(cfg)?).context("escribiendo config")?;
    Ok(())
}

// ---------- HTTP helpers ----------

struct Client {
    http: reqwest::Client,
    endpoint: String,
    session: Option<String>,
}

impl Client {
    fn new(endpoint: String, session: Option<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { http, endpoint, session })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.endpoint.trim_end_matches('/'), path)
    }

    fn auth_header(&self) -> Option<(&'static str, String)> {
        self.session
            .as_ref()
            .map(|s| ("Cookie", format!("faro_session={s}")))
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let mut req = self.http.get(self.url(path));
        if let Some((k, v)) = self.auth_header() {
            req = req.header(k, v);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!("401 — ejecuta `farocli login` primero");
        }
        if !status.is_success() {
            anyhow::bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
        }
        Ok(resp.json().await?)
    }

    async fn post_json<T: serde::de::DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let mut req = self.http.post(self.url(path)).json(body);
        if let Some((k, v)) = self.auth_header() {
            req = req.header(k, v);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("HTTP {}: {}", status, resp.text().await.unwrap_or_default());
        }
        Ok(resp.json().await?)
    }
}

// ---------- Comandos ----------

async fn cmd_login(client_endpoint: String, email_arg: Option<String>) -> Result<()> {
    let email = match email_arg {
        Some(e) => e,
        None => prompt("Email: ")?,
    };
    let password = rpassword::prompt_password("Password: ").context("leyendo password")?;
    let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;
    let endpoint = client_endpoint.trim_end_matches('/').to_string();

    let resp = http
        .post(format!("{endpoint}/api/v1/auth/login"))
        .json(&json!({ "email": email, "password": password }))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("login falló: HTTP {} — {}", resp.status(), resp.text().await.unwrap_or_default());
    }
    // Guardamos la cookie del primer response antes de consumir el body con
    // `.json()`, que toma `self` y nos deja sin acceso al header después.
    let cookie_from_first = extract_session_cookie(resp.headers());
    let body: Value = resp.json().await?;

    let session = if body.get("needs_totp").and_then(Value::as_bool).unwrap_or(false) {
        // 2FA: la cookie sólo se emite en la respuesta del segundo paso.
        let challenge = body
            .get("challenge_token")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("respuesta 2FA sin challenge_token"))?;
        let code = prompt("Código TOTP: ")?;
        let resp2 = http
            .post(format!("{endpoint}/api/v1/auth/login/2fa"))
            .json(&json!({ "challenge_token": challenge, "code": code.trim() }))
            .send()
            .await?;
        if !resp2.status().is_success() {
            anyhow::bail!("2FA falló: HTTP {} — {}", resp2.status(), resp2.text().await.unwrap_or_default());
        }
        extract_session_cookie(resp2.headers())
            .ok_or_else(|| anyhow!("la respuesta 2FA no trajo cookie faro_session"))?
    } else {
        cookie_from_first.ok_or_else(|| anyhow!("la respuesta de login no trajo cookie faro_session"))?
    };

    save_config(&StoredConfig {
        endpoint: Some(endpoint),
        session: Some(session),
    })?;
    println!("✓ Sesión guardada en {}", config_path()?.display());
    Ok(())
}

/// El servidor pone `faro_session` con HttpOnly. Eso es enforcement de
/// browsers — un cliente HTTP plano puede leer Set-Cookie sin problema.
fn extract_session_cookie(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find_map(|s| s.split(';').next()?.strip_prefix("faro_session="))
        .map(|s| s.to_string())
}

fn cmd_logout() -> Result<()> {
    let path = config_path()?;
    if path.exists() {
        std::fs::remove_file(&path).context("borrando config")?;
        println!("✓ Sesión local borrada");
    } else {
        println!("(no había sesión guardada)");
    }
    Ok(())
}

async fn cmd_logs(client: &Client, project: Option<&str>, args: LogsArgs) -> Result<()> {
    let min_sev = parse_severity(args.severity.as_deref())?;
    let mut qs: Vec<(String, String)> = Vec::new();
    if let Some(p) = project { qs.push(("project".into(), p.into())); }
    if let Some(s) = &args.service { qs.push(("service".into(), s.clone())); }
    if let Some(s) = min_sev { qs.push(("min_severity".into(), s.to_string())); }
    if let Some(q) = &args.query { qs.push(("query".into(), q.clone())); }

    if args.follow {
        return follow_logs(client, &qs).await;
    }

    let mins = parse_duration_minutes(&args.last)?;
    qs.push(("last_minutes".into(), mins.to_string()));
    qs.push(("limit".into(), args.limit.to_string()));
    let path = format!("/api/v1/logs?{}", encode_qs(&qs));
    let rows: Vec<Value> = client.get_json(&path).await?;
    // El endpoint devuelve DESC por timestamp; invertimos para imprimir
    // cronológicamente (lo último abajo, como en `tail`).
    for row in rows.iter().rev() {
        print_log(row, args.json);
    }
    Ok(())
}

async fn follow_logs(client: &Client, qs: &[(String, String)]) -> Result<()> {
    let path = format!("/api/v1/logs/live?{}", encode_qs(qs));
    let mut req = client.http.get(client.url(&path));
    if let Some((k, v)) = client.auth_header() {
        req = req.header(k, v);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} al abrir SSE", resp.status());
    }
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        buf.push_str(&String::from_utf8_lossy(&bytes));
        // SSE: mensajes separados por línea en blanco. Procesamos los que ya
        // están completos en el buffer y conservamos la cola para el próximo chunk.
        while let Some(idx) = buf.find("\n\n") {
            let raw = buf[..idx].to_string();
            buf.drain(..idx + 2);
            if let Some(data) = raw.lines().find_map(|l| l.strip_prefix("data: ")) {
                if let Ok(row) = serde_json::from_str::<Value>(data) {
                    print_log(&row, false);
                }
            }
        }
    }
    Ok(())
}

async fn cmd_services(client: &Client, project: Option<&str>) -> Result<()> {
    let path = match project {
        Some(p) => format!("/api/v1/services?project={}&last_minutes=60", urlencode(p)),
        None => "/api/v1/services?last_minutes=60".into(),
    };
    let rows: Vec<Value> = client.get_json(&path).await?;
    println!("{:<32} {:>10} {:>10}  last seen", "service", "logs", "errors");
    for r in rows {
        let svc = r.get("service_name").and_then(Value::as_str).unwrap_or("?");
        let logs = r.get("log_count").and_then(Value::as_u64).unwrap_or(0);
        let errs = r.get("error_count").and_then(Value::as_u64).unwrap_or(0);
        let last = r.get("last_seen").and_then(Value::as_str).unwrap_or("");
        println!("{:<32} {:>10} {:>10}  {}", truncate(svc, 32), logs, errs, last);
    }
    Ok(())
}

async fn cmd_errors(client: &Client, project: Option<&str>, status: Option<String>) -> Result<()> {
    let mut qs: Vec<(String, String)> = vec![("last_minutes".into(), "1440".into())];
    if let Some(p) = project { qs.push(("project".into(), p.into())); }
    if let Some(s) = status { qs.push(("status".into(), s)); }
    let path = format!("/api/v1/errors?{}", encode_qs(&qs));
    let rows: Vec<Value> = client.get_json(&path).await?;
    println!("{:<24} {:>8} {:<10} {:<20} message", "fingerprint", "count", "status", "service");
    for r in rows {
        let fp = r.get("fingerprint").and_then(Value::as_str).unwrap_or("");
        let n = r.get("event_count").and_then(Value::as_u64).unwrap_or(0);
        let st = r.get("status").and_then(Value::as_str).unwrap_or("");
        let svc = r.get("service_name").and_then(Value::as_str).unwrap_or("");
        let msg = r.get("message").and_then(Value::as_str).unwrap_or("");
        println!(
            "{:<24} {:>8} {:<10} {:<20} {}",
            &fp[..fp.len().min(24)],
            n, st, truncate(svc, 20), truncate(msg, 80)
        );
    }
    Ok(())
}

async fn cmd_monitor_list(client: &Client, project: Option<&str>) -> Result<()> {
    let path = match project {
        Some(p) => format!("/api/v1/monitors?project={}", urlencode(p)),
        None => "/api/v1/monitors".into(),
    };
    let rows: Vec<Value> = client.get_json(&path).await?;
    println!("{:<28} {:<6} {:>6}  url", "name", "enabl", "every");
    for r in rows {
        let name = r.get("name").and_then(Value::as_str).unwrap_or("?");
        let url = r.get("url").and_then(Value::as_str).unwrap_or("");
        let interval = r.get("interval_seconds").and_then(Value::as_u64).unwrap_or(0);
        let enabled = r.get("enabled").and_then(Value::as_u64).unwrap_or(0);
        println!("{:<28} {:<6} {:>5}s  {}", truncate(name, 28), if enabled == 1 { "yes" } else { "no" }, interval, url);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_monitor_create(
    client: &Client,
    project: Option<&str>,
    name: String,
    url: String,
    method: String,
    interval: u32,
    timeout: u32,
) -> Result<()> {
    let body = json!({
        "name": name,
        "project": project.unwrap_or("default"),
        "method": method.to_uppercase(),
        "url": url,
        "interval_seconds": interval,
        "timeout_seconds": timeout,
        "expected_status_min": 200,
        "expected_status_max": 299,
        "enabled": 1,
    });
    let created: Value = client.post_json("/api/v1/monitors", &body).await?;
    println!("✓ Creado: {}", created.get("id").and_then(Value::as_str).unwrap_or("?"));
    Ok(())
}

// ---------- Helpers ----------

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

/// Severidad: cualquier prefijo case-insensitive del nivel OTel. Devuelve el
/// número que entiende el endpoint (mismo mapeo que `LogRow::severity_from_text`).
fn parse_severity(s: Option<&str>) -> Result<Option<u8>> {
    let Some(s) = s else { return Ok(None) };
    let n = match s.to_ascii_uppercase().as_str() {
        "TRACE" => 1,
        "DEBUG" => 5,
        "INFO" => 9,
        "WARN" | "WARNING" => 13,
        "ERROR" | "ERR" => 17,
        "FATAL" | "CRITICAL" => 21,
        other => anyhow::bail!("severidad desconocida: {other}"),
    };
    Ok(Some(n))
}

/// Acepta `5m`, `1h`, `24h`, `7d`. Devuelve minutos. Mantiene la sintaxis del
/// dashboard para que `--last` se sienta familiar.
fn parse_duration_minutes(s: &str) -> Result<i64> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: i64 = num.parse().context("número inválido en --last")?;
    let mult = match unit {
        "m" | "" => 1,
        "h" => 60,
        "d" => 60 * 24,
        other => anyhow::bail!("unidad inválida en --last: {other} (usa m/h/d)"),
    };
    Ok(n * mult)
}

fn encode_qs(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn urlencode(s: &str) -> String {
    // Encoder minimalista — la API solo necesita escapar `&`, `=`, `#`, espacios
    // y caracteres no-ASCII. Para todo lo demás es transparente.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn truncate(s: &str, n: usize) -> &str {
    if s.len() <= n { s } else { &s[..n] }
}

fn print_log(row: &Value, as_json: bool) {
    if as_json {
        if let Ok(s) = serde_json::to_string(row) { println!("{s}"); }
        return;
    }
    let ts = row.get("timestamp").and_then(Value::as_str).unwrap_or("");
    let sev = row.get("severity_text").and_then(Value::as_str).unwrap_or("INFO");
    let svc = row.get("service_name").and_then(Value::as_str).unwrap_or("?");
    let body = row.get("body").and_then(Value::as_str).unwrap_or("");
    // Color por severidad — apagado si NO_COLOR está o stdout no es TTY.
    let color = std::env::var_os("NO_COLOR").is_none();
    let (open, close) = if color {
        match sev {
            "ERROR" | "ERR" | "FATAL" | "CRITICAL" => ("\x1b[31m", "\x1b[0m"),
            "WARN" | "WARNING" => ("\x1b[33m", "\x1b[0m"),
            "INFO" => ("\x1b[36m", "\x1b[0m"),
            _ => ("\x1b[90m", "\x1b[0m"),
        }
    } else {
        ("", "")
    };
    println!("{} {open}[{:<5}]{close} {} — {}", &ts[..ts.len().min(23)], sev, svc, body);
}

// ---------- main ----------

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let stored = load_config();

    // El --endpoint del flag tiene prioridad. Si no, lo guardado. Si tampoco,
    // el default. El proyecto se pasa por flag o env, no se persiste —
    // queremos que cambiar de proyecto sea explícito.
    let endpoint = cli
        .endpoint
        .clone()
        .or(stored.endpoint.clone())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());

    match cli.cmd {
        Cmd::Login { email } => cmd_login(endpoint, email).await,
        Cmd::Logout => cmd_logout(),
        Cmd::Logs(args) => {
            let client = Client::new(endpoint, stored.session.clone())?;
            cmd_logs(&client, cli.project.as_deref(), args).await
        }
        Cmd::Services => {
            let client = Client::new(endpoint, stored.session.clone())?;
            cmd_services(&client, cli.project.as_deref()).await
        }
        Cmd::Errors { status } => {
            let client = Client::new(endpoint, stored.session.clone())?;
            cmd_errors(&client, cli.project.as_deref(), status).await
        }
        Cmd::Monitor(MonitorCmd::List) => {
            let client = Client::new(endpoint, stored.session.clone())?;
            cmd_monitor_list(&client, cli.project.as_deref()).await
        }
        Cmd::Monitor(MonitorCmd::Create { name, url, method, interval, timeout }) => {
            let client = Client::new(endpoint, stored.session.clone())?;
            cmd_monitor_create(&client, cli.project.as_deref(), name, url, method, interval, timeout).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_units() {
        assert_eq!(parse_duration_minutes("5m").unwrap(), 5);
        assert_eq!(parse_duration_minutes("1h").unwrap(), 60);
        assert_eq!(parse_duration_minutes("24h").unwrap(), 1440);
        assert_eq!(parse_duration_minutes("7d").unwrap(), 10080);
        assert_eq!(parse_duration_minutes("30").unwrap(), 30);
        assert!(parse_duration_minutes("5x").is_err());
    }

    #[test]
    fn severity_parsing() {
        assert_eq!(parse_severity(Some("error")).unwrap(), Some(17));
        assert_eq!(parse_severity(Some("WARN")).unwrap(), Some(13));
        assert_eq!(parse_severity(None).unwrap(), None);
        assert!(parse_severity(Some("nope")).is_err());
    }

    #[test]
    fn url_encode_keeps_unreserved() {
        assert_eq!(urlencode("hola-mundo_1.0~"), "hola-mundo_1.0~");
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
    }
}
