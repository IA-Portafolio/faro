//! 2FA TOTP (RFC 6238) y recovery codes.
//!
//! - Secreto: 20 bytes random (HOTP-RFC4226 mínimo recomendado) almacenado como Base32.
//! - Verificación: SHA-1 (default de RFC 6238 y de toda app TOTP en producción), 6 dígitos,
//!   step 30 s, skew = 1 (acepta el período previo y el siguiente → ventana de 90 s).
//!   Relojes desincronizados son comunes; 90 s es el balance estándar entre UX y seguridad.
//! - Rate limit verificación: 5 intentos / minuto / user. Sin esto los 6 dígitos (~10^6
//!   códigos) son brute-forceables vía el endpoint en minutos.
//! - Recovery codes: 10 códigos de 10 chars cada uno (alfabeto sin 0/O/1/I/L para evitar
//!   confusión visual). Se muestran al user UNA vez al activar 2FA; sólo SHA-256 en DB;
//!   cada uso marca `used_at` y NO se reactiva.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use rand::rngs::SysRng;
use rand::TryRng;
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

/// Emisor que aparece en la entrada del authenticator. Visible al user.
pub const TOTP_ISSUER: &str = "Faro";

const TOTP_DIGITS: usize = 6;
const TOTP_SKEW: u8 = 1; // ±1 período = ventana de 90 s
const TOTP_STEP_SECS: u64 = 30;
const TOTP_SECRET_BYTES: usize = 20; // recomendación RFC 4226 §4

/// Alfabeto Crockford-ish: sin 0/O/1/I/L para que un user que copia a mano no se confunda.
const RECOVERY_CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const RECOVERY_CODE_LEN: usize = 10;
const RECOVERY_CODE_COUNT: usize = 10;

/// Genera un secreto TOTP nuevo (20 bytes random) y devuelve su representación Base32 —
/// éste es el string que el authenticator necesita.
pub fn generate_secret_base32() -> String {
    let mut bytes = vec![0u8; TOTP_SECRET_BYTES];
    // rand 0.10: SysRng (ex-OsRng) impl TryRng — usar try_fill_bytes.
    SysRng.try_fill_bytes(&mut bytes).expect("OS RNG failed");
    Secret::Raw(bytes).to_encoded().to_string()
}

/// Construye un `TOTP` a partir del secreto Base32 almacenado para hacer `verify`/`url`.
fn build_totp(secret_base32: &str, account: &str) -> Result<TOTP> {
    let raw = Secret::Encoded(secret_base32.to_string())
        .to_bytes()
        .map_err(|e| anyhow!("secreto TOTP inválido en DB: {e:?}"))?;
    TOTP::new(
        Algorithm::SHA1,
        TOTP_DIGITS,
        TOTP_SKEW,
        TOTP_STEP_SECS,
        raw,
        Some(TOTP_ISSUER.to_string()),
        account.to_string(),
    )
    .context("construyendo TOTP")
}

/// otpauth:// URL canónico, listo para meter en un QR.
pub fn otpauth_url(secret_base32: &str, account_email: &str) -> Result<String> {
    Ok(build_totp(secret_base32, account_email)?.get_url())
}

/// SVG inline del QR. El frontend lo embebe directamente; no requiere lib JS.
/// Tamaño en pixels para el atributo width/height — visualmente 256 px funciona bien.
pub fn otpauth_qr_svg(secret_base32: &str, account_email: &str) -> Result<String> {
    let url = otpauth_url(secret_base32, account_email)?;
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::M)
        .map_err(|e| anyhow!("error generando QR: {e}"))?;
    let svg = code
        .render::<svg::Color<'_>>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(svg)
}

/// Valida un código TOTP de 6 dígitos contra el secreto, con skew ±1.
pub fn verify_totp(secret_base32: &str, account_email: &str, code: &str) -> Result<bool> {
    // El input puede traer espacios o guiones — Google Authenticator muestra "123 456".
    let trimmed: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if trimmed.len() != TOTP_DIGITS {
        return Ok(false);
    }
    let totp = build_totp(secret_base32, account_email)?;
    Ok(totp.check_current(&trimmed).unwrap_or(false))
}

// ---------- Recovery codes ----------

/// Hash que se guarda en DB. SHA-256 alcanza — los plaintexts son aleatorios y de
/// alta entropía (~50 bits), no necesitan Argon2 (que sería overkill y serializaría
/// el login por el costo del KDF).
pub fn hash_recovery_code(code: &str) -> String {
    let normalized = normalize_recovery_code(code);
    let mut h = Sha256::new();
    h.update(normalized.as_bytes());
    hex::encode(h.finalize())
}

/// Aplana guiones, espacios y minúsculas para que `ABCD-1234-EF` y `abcd1234ef`
/// coincidan con el hash del que el user copió de la pantalla.
pub fn normalize_recovery_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Genera el set completo de recovery codes (plaintext). El caller debe hashearlos
/// para persistir, y mostrar los plaintexts AL USER UNA SOLA VEZ.
pub fn generate_recovery_codes() -> Vec<String> {
    (0..RECOVERY_CODE_COUNT)
        .map(|_| generate_one_code())
        .collect()
}

fn generate_one_code() -> String {
    let mut buf = vec![0u8; RECOVERY_CODE_LEN];
    SysRng.try_fill_bytes(&mut buf).expect("OS RNG failed");
    let raw: String = buf
        .into_iter()
        .map(|b| RECOVERY_CODE_ALPHABET[(b as usize) % RECOVERY_CODE_ALPHABET.len()] as char)
        .collect();
    // Formato visual `ABCDE-FGHIJ` — más fácil de copiar y leer.
    format!("{}-{}", &raw[..5], &raw[5..])
}

// ---------- Rate limiter para verificación TOTP/recovery ----------

/// In-memory por proceso. Con un único nodo de backend (modelo de despliegue actual de
/// Faro) alcanza; si en el futuro hay más nodos, esto migra a Redis con la misma API.
#[derive(Clone, Default)]
pub struct TotpRateLimiter {
    inner: Arc<Mutex<HashMap<Uuid, Vec<Instant>>>>,
}

const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const RATE_LIMIT_MAX_ATTEMPTS: usize = 5;

impl TotpRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra un intento. Devuelve `true` si está permitido; `false` si pasó el cap.
    pub fn check_and_record(&self, user_id: Uuid) -> bool {
        let now = Instant::now();
        let cutoff = now.checked_sub(RATE_LIMIT_WINDOW).unwrap_or(now);
        let mut guard = self.inner.lock();
        let attempts = guard.entry(user_id).or_default();
        attempts.retain(|t| *t >= cutoff);
        if attempts.len() >= RATE_LIMIT_MAX_ATTEMPTS {
            return false;
        }
        attempts.push(now);
        true
    }

    /// Limpia el registro de intentos del user (se llama al verificar OK).
    pub fn clear(&self, user_id: Uuid) {
        self.inner.lock().remove(&user_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_codes_have_expected_shape() {
        let codes = generate_recovery_codes();
        assert_eq!(codes.len(), 10);
        for c in &codes {
            assert_eq!(c.len(), 11); // 10 chars + 1 guión
            assert!(c.contains('-'));
            let normalized = normalize_recovery_code(c);
            assert_eq!(normalized.len(), 10);
            for ch in normalized.chars() {
                assert!(RECOVERY_CODE_ALPHABET.contains(&(ch as u8)));
            }
        }
    }

    #[test]
    fn normalize_strips_separators_and_lowercases() {
        assert_eq!(normalize_recovery_code("abcd-efgh-12"), "ABCDEFGH12");
        assert_eq!(normalize_recovery_code("  AB CD  "), "ABCD");
    }

    #[test]
    fn hash_is_deterministic_after_normalization() {
        let a = hash_recovery_code("abcd-1234-ef");
        let b = hash_recovery_code("ABCD1234EF");
        assert_eq!(a, b);
    }

    #[test]
    fn totp_secret_is_base32() {
        let s = generate_secret_base32();
        // Base32 sin padding produce sólo [A-Z2-7]
        for c in s.chars() {
            assert!(c.is_ascii_alphabetic() || ('2'..='7').contains(&c));
        }
    }

    #[test]
    fn totp_roundtrip_verifies_own_code() {
        let secret = generate_secret_base32();
        let totp = build_totp(&secret, "test@example.com").unwrap();
        let code = totp.generate_current().unwrap();
        assert!(verify_totp(&secret, "test@example.com", &code).unwrap());
        // Espaciado a la "Google Authenticator"
        let spaced = format!("{} {}", &code[..3], &code[3..]);
        assert!(verify_totp(&secret, "test@example.com", &spaced).unwrap());
    }

    #[test]
    fn rate_limiter_blocks_after_cap() {
        let rl = TotpRateLimiter::new();
        let uid = Uuid::new_v4();
        for _ in 0..RATE_LIMIT_MAX_ATTEMPTS {
            assert!(rl.check_and_record(uid));
        }
        assert!(!rl.check_and_record(uid));
    }
}
