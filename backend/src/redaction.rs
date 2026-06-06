//! PII redaction al ingestar.
//!
//! Aplicación por proyecto, configurable desde el dashboard. Una vez una regla está
//! activa, los rows entran ya redactados a ClickHouse — no hay forma de "des-redactar"
//! después porque NO guardamos el original (a propósito; el costo de almacenarlo es
//! exactamente lo que queremos evitar).
//!
//! **Built-ins**:
//!   - `email`        — direcciones de correo
//!   - `jwt`          — tokens estilo `xxx.yyy.zzz` con base64url
//!   - `credit_card`  — secuencias de 13-19 dígitos con o sin separadores; sin Luhn,
//!                      asumimos que un FP ocasional (un número de orden) es preferible
//!                      al FN de filtrar una tarjeta real
//!   - `bearer`       — `Authorization: Bearer xxx` y variantes
//!   - `password_kv`  — `password=...`, `pwd=...`, `pass=...` en logs estilo `key=value`
//!   - `apikey_kv`    — `api_key=...`, `apikey=...`, `secret=...`, `token=...`
//!   - `ip`           — IPv4 (IPv6 omitido por ahora; el regex es pesado y los FPs
//!                      con MAC addresses son comunes)
//!
//! **Custom rules**: regex + replacement, validadas al guardar para evitar ReDoS obvios
//! (sin nested quantifiers, sin backtracking exponencial — el crate `regex` ya no soporta
//! lookaround, así que la mayoría de patrones peligrosos quedan fuera de fábrica).

use std::borrow::Cow;
use std::sync::Arc;

use regex::Regex;
use serde::{Deserialize, Serialize};

pub const REDACTED: &str = "[REDACTED]";

/// Lista cerrada de built-ins disponibles. Se identifican por slug en la config JSON
/// para que la representación en DB no cambie si renombramos los enums en código.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Email,
    Jwt,
    CreditCard,
    Bearer,
    PasswordKv,
    ApikeyKv,
    Ip,
}

impl Builtin {
    pub const ALL: &'static [Builtin] = &[
        Builtin::Email,
        Builtin::Jwt,
        Builtin::CreditCard,
        Builtin::Bearer,
        Builtin::PasswordKv,
        Builtin::ApikeyKv,
        Builtin::Ip,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Builtin::Email => "email",
            Builtin::Jwt => "jwt",
            Builtin::CreditCard => "credit_card",
            Builtin::Bearer => "bearer",
            Builtin::PasswordKv => "password_kv",
            Builtin::ApikeyKv => "apikey_kv",
            Builtin::Ip => "ip",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|b| b.slug() == s)
    }

    /// Regex source. Mantenemos los patrones simples para el motor `regex` (sin
    /// lookaround) y conscientes de UTF-8.
    fn pattern(self) -> &'static str {
        match self {
            // RFC 5322-simplificado: suficiente para `user@host.tld`. Evitamos
            // el patrón "oficial" porque introduce backtracking horrible.
            Builtin::Email => r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}",
            // JWT: tres segmentos base64url separados por punto. Mínimo 8 chars
            // por segmento para no matchear cosas como `a.b.c` triviales.
            Builtin::Jwt => r"eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}",
            // 13-19 dígitos opcionalmente separados por espacios o guiones.
            // `\b` evita matchear dentro de identifiers más largos.
            Builtin::CreditCard => r"\b(?:\d[ \-]?){13,18}\d\b",
            // Authorization: Bearer xxxx, también `Bearer xxx` en texto libre.
            Builtin::Bearer => r"(?i)(bearer|basic)\s+[A-Za-z0-9._\-+/=]{8,}",
            // `password=xxx`, `password: xxx`, `"password":"xxx"`, `pwd=xxx`. Comillas
            // opcionales alrededor de la key cubren los logs JSON-estructurados, que son
            // lo más común en stacks modernos (slog, zap, pino, structlog). Las variantes
            // se enumeran explícitamente: `pass(?:word|wd)?` no matchea `pwd` solo
            // (typo común que dejaba pasar `PWD: hunter2`).
            Builtin::PasswordKv => {
                r#"(?i)"?(password|passwd|pwd|pass|secret_word|kennwort)"?\s*[:=]\s*("[^"]*"|'[^']*'|[^\s,;}\]&]+)"#
            }
            // api_key, apikey, access_token, auth_token, secret, token (no JWT,
            // ya cubierto arriba). El `token` "pelado" va al final de la
            // alternación: las claves más largas (`access_token`, `auth_token`)
            // ya matchean por sí mismas, y un campo llamado simplemente `token`
            // —que el catálogo de la UI promete cubrir— también debe redactarse.
            // El `[:=]` obligatorio evita falsos positivos como `token_count`.
            Builtin::ApikeyKv => {
                r#"(?i)"?(api[_\-]?key|apikey|access[_\-]?token|auth[_\-]?token|secret|token)"?\s*[:=]\s*("[^"]*"|'[^']*'|[^\s,;}\]&]+)"#
            }
            // IPv4. Evitamos rangos exactos: cualquier 4 grupos de 1-3 dígitos.
            // Pierde un poco de precisión (matchea 999.999.999.999) a cambio de
            // un patrón sin backtracking.
            Builtin::Ip => r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b",
        }
    }

    /// Replacement. Para `*_kv` preservamos la **key** capturada (group $1) para que
    /// el log siga indicando QUÉ se redactó, sin las comillas/separador originales
    /// que complican intentar reconstruir el formato exacto en JSON vs. key=value.
    /// Para Bearer/Basic preservamos el scheme.
    fn replacement(self) -> &'static str {
        match self {
            Builtin::PasswordKv | Builtin::ApikeyKv => "$1=[REDACTED]",
            Builtin::Bearer => "$1 [REDACTED]",
            _ => REDACTED,
        }
    }
}

/// Config serializada en `faro.projects.redaction_rules` (JSON crudo).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RedactionConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Slugs de built-ins activos. Slugs desconocidos se ignoran (forward-compat).
    #[serde(default)]
    pub builtins: Vec<String>,
    #[serde(default)]
    pub custom: Vec<CustomRule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomRule {
    pub name: String,
    pub pattern: String,
    #[serde(default = "default_replacement")]
    pub replacement: String,
}

fn default_replacement() -> String {
    REDACTED.to_string()
}

/// Catálogo de built-ins para la UI. Sólo metadata; los regex viven en `Builtin`.
#[derive(Serialize)]
pub struct BuiltinInfo {
    pub slug: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

pub fn builtin_catalog() -> Vec<BuiltinInfo> {
    vec![
        BuiltinInfo {
            slug: "email",
            label: "Email",
            description: "user@example.com → [REDACTED]",
        },
        BuiltinInfo {
            slug: "jwt",
            label: "JWT",
            description: "Tokens eyJ... de tres segmentos",
        },
        BuiltinInfo {
            slug: "credit_card",
            label: "Tarjeta",
            description: "13-19 dígitos con o sin separadores",
        },
        BuiltinInfo {
            slug: "bearer",
            label: "Bearer / Basic auth",
            description: "Authorization: Bearer xxx",
        },
        BuiltinInfo {
            slug: "password_kv",
            label: "password=...",
            description: "password / pwd / pass en formato key=value",
        },
        BuiltinInfo {
            slug: "apikey_kv",
            label: "api_key=...",
            description: "api_key / apikey / secret / token en key=value",
        },
        BuiltinInfo {
            slug: "ip",
            label: "IPv4",
            description: "Direcciones IPv4",
        },
    ]
}

// ---------- Compilación ----------

/// Set de reglas compiladas, listas para `.apply(text)`. `Arc` para clonar barato
/// entre el caller (cache) y el ingest path.
#[derive(Clone, Default)]
pub struct CompiledRules {
    inner: Arc<CompiledInner>,
}

#[derive(Default)]
struct CompiledInner {
    rules: Vec<CompiledRule>,
}

struct CompiledRule {
    re: Regex,
    replacement: String,
}

impl CompiledRules {
    /// `None` si el config no es JSON válido o si `enabled = false` — el ingest
    /// path entonces no hace ningún trabajo extra.
    pub fn from_config_str(raw: &str) -> Option<Self> {
        if raw.trim().is_empty() {
            return None;
        }
        let cfg: RedactionConfig = serde_json::from_str(raw).ok()?;
        Self::from_config(&cfg)
    }

    pub fn from_config(cfg: &RedactionConfig) -> Option<Self> {
        if !cfg.enabled {
            return None;
        }
        let mut rules = Vec::new();
        for slug in &cfg.builtins {
            if let Some(b) = Builtin::from_slug(slug) {
                if let Ok(re) = Regex::new(b.pattern()) {
                    rules.push(CompiledRule {
                        re,
                        replacement: b.replacement().to_string(),
                    });
                }
            }
        }
        for r in &cfg.custom {
            // Si el regex no compila, lo skippeamos en lugar de fallar la carga del
            // proyecto entero — la UI ya tuvo que validarlo al guardar.
            if let Ok(re) = Regex::new(&r.pattern) {
                rules.push(CompiledRule {
                    re,
                    replacement: r.replacement.clone(),
                });
            }
        }
        if rules.is_empty() {
            return None;
        }
        Some(Self {
            inner: Arc::new(CompiledInner { rules }),
        })
    }

    /// Aplica todos los regex en orden. Devuelve `Cow::Borrowed` si ningún regex
    /// matcheó — el caller no necesita reasignar el string en el caso común sin PII.
    pub fn apply<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut cur: Cow<'a, str> = Cow::Borrowed(text);
        for rule in &self.inner.rules {
            // `Regex::replace_all` ya devuelve Cow — si el patrón no matchea no
            // copia el string.
            match rule.re.replace_all(&cur, rule.replacement.as_str()) {
                Cow::Borrowed(_) => { /* sin cambios */ }
                Cow::Owned(new) => cur = Cow::Owned(new),
            }
        }
        cur
    }

    /// Aplica in-place sobre `String`, evitando una asignación si no hubo cambios.
    pub fn apply_in_place(&self, s: &mut String) {
        if let Cow::Owned(new) = self.apply(s.as_str()) {
            *s = new;
        }
    }

    /// Aplica a los **valores** de un `AttrMap` (no a las keys; las keys son nombres
    /// de campo que el dashboard usa para filtrar y agrupar — redactarlos rompería
    /// queries silenciosamente).
    pub fn apply_to_attrs(&self, attrs: &mut crate::storage::AttrMap) {
        for v in attrs.values_mut() {
            self.apply_in_place(v);
        }
    }
}

// ---------- Validación de regex para el endpoint PUT ----------

/// Compila el regex y devuelve un error legible si falla. El motor `regex` rechaza
/// lookaround y backreferences, así que la familia de ReDoS clásica está cubierta
/// por el propio compile. Imponemos además un tope de longitud para evitar patrones
/// monstruosos por accidente o por abuso.
const MAX_CUSTOM_PATTERN_LEN: usize = 2048;

pub fn validate_custom_pattern(pattern: &str) -> Result<(), String> {
    if pattern.is_empty() {
        return Err("el patrón no puede estar vacío".into());
    }
    if pattern.len() > MAX_CUSTOM_PATTERN_LEN {
        return Err(format!(
            "patrón demasiado largo ({} bytes, máximo {})",
            pattern.len(),
            MAX_CUSTOM_PATTERN_LEN
        ));
    }
    Regex::new(pattern).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(builtins: &[&str]) -> RedactionConfig {
        RedactionConfig {
            enabled: true,
            builtins: builtins.iter().map(|s| s.to_string()).collect(),
            custom: vec![],
        }
    }

    fn apply(c: &CompiledRules, s: &str) -> String {
        c.apply(s).into_owned()
    }

    #[test]
    fn disabled_returns_none() {
        let mut c = RedactionConfig::default();
        c.enabled = false;
        c.builtins = vec!["email".into()];
        assert!(CompiledRules::from_config(&c).is_none());
    }

    #[test]
    fn empty_returns_none() {
        let c = RedactionConfig {
            enabled: true,
            builtins: vec![],
            custom: vec![],
        };
        assert!(CompiledRules::from_config(&c).is_none());
    }

    #[test]
    fn email_redacted() {
        let c = CompiledRules::from_config(&cfg(&["email"])).unwrap();
        assert_eq!(
            apply(&c, "contact alice@example.com today"),
            "contact [REDACTED] today"
        );
    }

    #[test]
    fn no_match_returns_borrowed() {
        let c = CompiledRules::from_config(&cfg(&["email"])).unwrap();
        let input = "plain log with no PII";
        let out = c.apply(input);
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn password_kv_preserves_key() {
        let c = CompiledRules::from_config(&cfg(&["password_kv"])).unwrap();
        // Estilo key=value tradicional
        assert_eq!(
            apply(&c, "user logged in password=hunter2 ok"),
            "user logged in password=[REDACTED] ok"
        );
        // Estilo JSON — las comillas alrededor de la key sí matchean por el `"?` del regex.
        // El replacement normaliza a `password=[REDACTED]` (perdemos las comillas/dos puntos
        // originales pero ganamos consistencia).
        assert_eq!(
            apply(&c, r#"{"password":"s3cret"}"#),
            r#"{password=[REDACTED]}"#
        );
        // Variantes y case-insensitive
        assert_eq!(apply(&c, "PWD: abc123"), "PWD=[REDACTED]");
    }

    #[test]
    fn apikey_kv_covers_documented_keys() {
        let c = CompiledRules::from_config(&cfg(&["apikey_kv"])).unwrap();
        // El catálogo de la UI promete: api_key / apikey / secret / token.
        assert_eq!(apply(&c, "api_key=abc123"), "api_key=[REDACTED]");
        assert_eq!(apply(&c, "apikey: abc123"), "apikey=[REDACTED]");
        assert_eq!(apply(&c, "access_token=abc123"), "access_token=[REDACTED]");
        assert_eq!(apply(&c, "auth_token=abc123"), "auth_token=[REDACTED]");
        assert_eq!(apply(&c, "secret=abc123"), "secret=[REDACTED]");
        // Un campo llamado simplemente `token` — antes se filtraba (regresión).
        assert_eq!(apply(&c, "token=abc123"), "token=[REDACTED]");
        assert_eq!(apply(&c, r#"{"token":"s3cret"}"#), r#"{token=[REDACTED]}"#);
    }

    #[test]
    fn apikey_kv_does_not_over_redact_token_substrings() {
        let c = CompiledRules::from_config(&cfg(&["apikey_kv"])).unwrap();
        // `token_count` no es una clave de secreto: el `[:=]` obligatorio justo
        // después de la clave evita el falso positivo (no hay `:`/`=` tras `token`).
        assert_eq!(apply(&c, "token_count=5"), "token_count=5");
    }

    #[test]
    fn bearer_preserves_scheme() {
        let c = CompiledRules::from_config(&cfg(&["bearer"])).unwrap();
        assert_eq!(
            apply(&c, "Authorization: Bearer abc.def.ghi"),
            "Authorization: Bearer [REDACTED]"
        );
    }

    #[test]
    fn jwt_redacted() {
        let c = CompiledRules::from_config(&cfg(&["jwt"])).unwrap();
        let s = "token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c rest";
        assert!(apply(&c, s).contains("[REDACTED]"));
    }

    #[test]
    fn credit_card_redacted() {
        let c = CompiledRules::from_config(&cfg(&["credit_card"])).unwrap();
        assert_eq!(
            apply(&c, "card 4111-1111-1111-1111 expires"),
            "card [REDACTED] expires"
        );
        assert_eq!(
            apply(&c, "card 4111111111111111 expires"),
            "card [REDACTED] expires"
        );
    }

    #[test]
    fn ip_redacted() {
        let c = CompiledRules::from_config(&cfg(&["ip"])).unwrap();
        assert_eq!(apply(&c, "from 192.168.0.1 ok"), "from [REDACTED] ok");
    }

    #[test]
    fn custom_rule_applied() {
        let cfg = RedactionConfig {
            enabled: true,
            builtins: vec![],
            custom: vec![CustomRule {
                name: "ssn".into(),
                pattern: r"\b\d{3}-\d{2}-\d{4}\b".into(),
                replacement: "[SSN]".into(),
            }],
        };
        let c = CompiledRules::from_config(&cfg).unwrap();
        assert_eq!(apply(&c, "ssn 123-45-6789 ok"), "ssn [SSN] ok");
    }

    #[test]
    fn invalid_regex_in_custom_is_skipped() {
        let cfg = RedactionConfig {
            enabled: true,
            builtins: vec!["email".into()],
            custom: vec![CustomRule {
                name: "bad".into(),
                pattern: r"[invalid".into(),
                replacement: "[X]".into(),
            }],
        };
        // El builtin email sigue funcionando; la regla custom inválida se ignora.
        let c = CompiledRules::from_config(&cfg).unwrap();
        assert_eq!(apply(&c, "a@b.cd"), "[REDACTED]");
    }

    #[test]
    fn from_config_str_empty_is_none() {
        assert!(CompiledRules::from_config_str("").is_none());
        assert!(CompiledRules::from_config_str("   ").is_none());
        assert!(CompiledRules::from_config_str("{not json").is_none());
    }

    #[test]
    fn validate_rejects_empty_and_too_long() {
        assert!(validate_custom_pattern("").is_err());
        assert!(validate_custom_pattern(&"a".repeat(MAX_CUSTOM_PATTERN_LEN + 1)).is_err());
        assert!(validate_custom_pattern("[invalid").is_err());
        assert!(validate_custom_pattern(r"\d+").is_ok());
    }

    #[test]
    fn apply_to_attrs_only_touches_values() {
        let c = CompiledRules::from_config(&cfg(&["email"])).unwrap();
        let mut m = crate::storage::AttrMap::new();
        m.insert("user.email".into(), "alice@example.com".into());
        m.insert("note".into(), "no pii here".into());
        c.apply_to_attrs(&mut m);
        assert_eq!(m.get("user.email").unwrap(), "[REDACTED]");
        assert_eq!(m.get("note").unwrap(), "no pii here");
        // La key "user.email" no se modifica.
        assert!(m.contains_key("user.email"));
    }
}
