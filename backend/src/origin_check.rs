//! Whitelist de orígenes para el RUM/browser SDK.
//!
//! ### Modelo de amenaza
//! El token de ingesta del SDK browser es público — viaja en el bundle JS que
//! sirve el cliente, así que cualquiera con DevTools puede leerlo. Sin
//! verificación adicional, un atacante puede inflar las métricas de un proyecto
//! desde un dominio arbitrario o quemar la cuota de ingesta.
//!
//! ### Mitigación
//! Validar el header `Origin` (que el browser SIEMPRE inyecta y NO puede ser
//! suplantado por código JS — sólo lo controla el browser) contra una whitelist
//! por proyecto.
//!
//! ### Política
//! - Lista vacía (`enabled=false` o sin entries) → fail-open: cualquier `Origin`
//!   pasa. Backward compat con proyectos creados antes de la feature.
//! - Lista con entries + request CON `Origin` → debe matchear o se rechaza 403.
//! - Lista con entries + request SIN `Origin` → pasa (es un cliente server-side
//!   como `@iaportafolio/node`; el bearer del proyecto ya autentica).
//!
//! ### Sintaxis aceptada
//! - Exacto: `https://app.example.com`, `http://localhost:3000`
//! - Wildcard de un nivel de subdominio: `https://*.example.com`
//!   matchea `https://foo.example.com` pero NO `https://example.com`
//!   ni `https://foo.bar.example.com`. (Un solo `*` en el primer label,
//!   sin permitir subdominios anidados — evita el clásico bypass por
//!   `evil.attacker.com.example.com` mal parseado).
//!
//! Comparación case-insensitive en scheme + host. No se hace canonicalización
//! de IDN ni de puertos por defecto: `https://example.com` y `https://example.com:443`
//! son orígenes distintos a los ojos del browser y de esta whitelist.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Config serializada en `faro.projects.allowed_origins`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OriginConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub origins: Vec<String>,
}

/// Regla compilada. Mantenemos scheme y host separados para evitar string-compares
/// repetidos en el path caliente del ingest.
#[derive(Clone, Debug)]
enum Rule {
    /// Match exacto contra `scheme://host[:port]` normalizado a minúsculas.
    Exact(String),
    /// `scheme` (minúsculas) + sufijo del host con el punto incluido (e.g. `.example.com`).
    /// Matchea `<algo>.example.com` con `<algo>` no vacío y sin más puntos.
    WildcardSubdomain {
        scheme: String,
        dot_suffix: String,
        port: Option<u16>,
    },
}

#[derive(Clone, Default)]
pub struct OriginRuleset {
    inner: Arc<Vec<Rule>>,
}

impl OriginRuleset {
    /// `None` si no hay reglas activas (la lista está vacía o `enabled=false`).
    /// El ingest path interpreta `None` como "no chequear".
    pub fn from_config_str(raw: &str) -> Option<Self> {
        if raw.trim().is_empty() {
            return None;
        }
        let cfg: OriginConfig = serde_json::from_str(raw).ok()?;
        Self::from_config(&cfg)
    }

    pub fn from_config(cfg: &OriginConfig) -> Option<Self> {
        if !cfg.enabled {
            return None;
        }
        let mut rules = Vec::new();
        for raw in &cfg.origins {
            if let Some(rule) = parse_rule(raw) {
                rules.push(rule);
            }
        }
        if rules.is_empty() {
            return None;
        }
        Some(Self {
            inner: Arc::new(rules),
        })
    }

    /// `true` si el `Origin` recibido matchea alguna regla.
    pub fn matches(&self, origin_header: &str) -> bool {
        let Some(o) = parse_origin_header(origin_header) else {
            return false;
        };
        for rule in self.inner.iter() {
            match rule {
                Rule::Exact(s) => {
                    if origin_eq(&o, s) {
                        return true;
                    }
                }
                Rule::WildcardSubdomain {
                    scheme,
                    dot_suffix,
                    port,
                } => {
                    if o.scheme != *scheme {
                        continue;
                    }
                    if o.port != *port {
                        continue;
                    }
                    // Tiene que haber al menos UN carácter antes del sufijo y NINGÚN punto
                    // en ese segmento — `*` matchea un único label.
                    let Some(prefix) = o.host.strip_suffix(dot_suffix.as_str()) else {
                        continue;
                    };
                    if prefix.is_empty() || prefix.contains('.') {
                        continue;
                    }
                    return true;
                }
            }
        }
        false
    }
}

/// Resultado intermedio del parsing del header `Origin` o de una regla exacta.
#[derive(Debug, PartialEq)]
struct ParsedOrigin {
    scheme: String,
    host: String,
    /// Sólo presente si el cliente lo declara explícitamente. `:443` en HTTPS
    /// y `:80` en HTTP son perfectamente válidos como Origin distinto al default.
    port: Option<u16>,
}

fn origin_eq(a: &ParsedOrigin, rule_canonical: &str) -> bool {
    let Some(b) = parse_origin_header(rule_canonical) else {
        return false;
    };
    a == &b
}

/// Reusable para regla exacta y para el header. Acepta `scheme://host[:port]`,
/// rechaza paths/query/fragment (eso NO es un origen válido).
fn parse_origin_header(s: &str) -> Option<ParsedOrigin> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("null") {
        return None;
    }
    let (scheme, rest) = s.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !is_valid_scheme(&scheme) {
        return None;
    }
    // Cualquier `/`, `?` o `#` después del host invalida el header como Origin
    // (el browser nunca debería enviarlos; si lo hace, es input sospechoso).
    if rest.contains('/') || rest.contains('?') || rest.contains('#') {
        return None;
    }
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) => {
            let port: u16 = p.parse().ok()?;
            (h.to_ascii_lowercase(), Some(port))
        }
        None => (rest.to_ascii_lowercase(), None),
    };
    if host.is_empty() {
        return None;
    }
    Some(ParsedOrigin { scheme, host, port })
}

fn is_valid_scheme(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

fn parse_rule(raw: &str) -> Option<Rule> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (scheme, rest) = raw.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !is_valid_scheme(&scheme) {
        return None;
    }
    // Una regla es un ORIGEN, no una URL. Cualquier path/query/fragment es
    // input mal armado del admin (e.g. pegó la URL completa del dashboard).
    if rest.contains('/') || rest.contains('?') || rest.contains('#') {
        return None;
    }
    let (host_part, port) = match rest.rsplit_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(port) => (h, Some(port)),
            Err(_) => return None,
        },
        None => (rest, None),
    };
    let host_part = host_part.to_ascii_lowercase();
    if host_part.is_empty() {
        return None;
    }
    if let Some(stripped) = host_part.strip_prefix("*.") {
        // `*.example.com` — wildcard de un solo label. El sufijo guardado incluye
        // el punto para que el matching sea un strip_suffix simple.
        if stripped.is_empty() || stripped.contains('*') {
            return None;
        }
        return Some(Rule::WildcardSubdomain {
            scheme,
            dot_suffix: format!(".{stripped}"),
            port,
        });
    }
    if host_part.contains('*') {
        // Wildcard en otra posición no soportado — sería más fácil equivocarse
        // (e.g. `https://api.*.com` permitiría `api.evil.com`).
        return None;
    }
    Some(Rule::Exact(format!(
        "{scheme}://{host_part}{}",
        port.map(|p| format!(":{p}")).unwrap_or_default()
    )))
}

/// Helper de validación para el endpoint PUT: chequea que cada regla parsee.
pub fn validate_pattern(raw: &str) -> Result<(), String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("entrada vacía".into());
    }
    if raw.len() > 256 {
        return Err(format!(
            "entrada demasiado larga ({} bytes, máximo 256)",
            raw.len()
        ));
    }
    parse_rule(raw).map(|_| ()).ok_or_else(|| {
        format!(
            "'{raw}' no es un origen válido. Formato esperado: scheme://host[:port], \
             opcionalmente con wildcard de un nivel como https://*.example.com"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rs(origins: &[&str]) -> OriginRuleset {
        let cfg = OriginConfig {
            enabled: true,
            origins: origins.iter().map(|s| s.to_string()).collect(),
        };
        OriginRuleset::from_config(&cfg).expect("compiles")
    }

    #[test]
    fn disabled_returns_none() {
        let cfg = OriginConfig {
            enabled: false,
            origins: vec!["https://a.com".into()],
        };
        assert!(OriginRuleset::from_config(&cfg).is_none());
    }

    #[test]
    fn empty_returns_none() {
        let cfg = OriginConfig {
            enabled: true,
            origins: vec![],
        };
        assert!(OriginRuleset::from_config(&cfg).is_none());
    }

    #[test]
    fn exact_match() {
        let r = rs(&["https://app.example.com"]);
        assert!(r.matches("https://app.example.com"));
        assert!(!r.matches("https://APP.example.com") == false); // case-insensitive
        assert!(r.matches("HTTPS://APP.EXAMPLE.COM")); // case-insensitive
        assert!(!r.matches("http://app.example.com")); // scheme distinto
        assert!(!r.matches("https://other.example.com"));
        assert!(!r.matches("https://app.example.com:8080")); // puerto explícito ≠ default
    }

    #[test]
    fn exact_with_port() {
        let r = rs(&["http://localhost:3000"]);
        assert!(r.matches("http://localhost:3000"));
        assert!(!r.matches("http://localhost:3001"));
        assert!(!r.matches("http://localhost")); // sin puerto NO matchea
    }

    #[test]
    fn wildcard_subdomain() {
        let r = rs(&["https://*.example.com"]);
        assert!(r.matches("https://foo.example.com"));
        assert!(r.matches("https://api.example.com"));
        // NO matchea el dominio raíz (sin subdominio)
        assert!(!r.matches("https://example.com"));
        // NO matchea subdominios anidados — evita bypass por wildcard greedy
        assert!(!r.matches("https://foo.bar.example.com"));
        // NO matchea otro dominio que termine igual
        assert!(!r.matches("https://attackerexample.com"));
        // NO matchea con puerto distinto al de la regla (regla sin puerto)
        assert!(!r.matches("https://foo.example.com:8443"));
    }

    #[test]
    fn wildcard_with_port() {
        let r = rs(&["https://*.example.com:8443"]);
        assert!(r.matches("https://foo.example.com:8443"));
        assert!(!r.matches("https://foo.example.com"));
        assert!(!r.matches("https://foo.example.com:8444"));
    }

    #[test]
    fn rejects_null_origin() {
        // Headers como `Origin: null` aparecen en sandboxes o redirects raros.
        // Rechazamos por seguridad — si la app legítima los necesita, la lista
        // tiene que incluirlo explícitamente (lo cual no soportamos por diseño).
        let r = rs(&["https://app.example.com"]);
        assert!(!r.matches("null"));
        assert!(!r.matches(""));
    }

    #[test]
    fn rejects_origin_with_path() {
        let r = rs(&["https://app.example.com"]);
        // Origin con path es input sospechoso (browser no lo envía así)
        assert!(!r.matches("https://app.example.com/admin"));
        assert!(!r.matches("https://app.example.com?x=1"));
    }

    #[test]
    fn invalid_rules_are_skipped() {
        // Las reglas inválidas se descartan al compilar; lo que queda funciona.
        let cfg = OriginConfig {
            enabled: true,
            origins: vec![
                "not-a-url".into(),
                "https://api.*.com".into(),       // wildcard mal puesto
                "https://app.example.com".into(), // ésta sí
            ],
        };
        let r = OriginRuleset::from_config(&cfg).expect("at least one valid");
        assert!(r.matches("https://app.example.com"));
    }

    #[test]
    fn validate_helper() {
        assert!(validate_pattern("https://app.example.com").is_ok());
        assert!(validate_pattern("https://*.example.com").is_ok());
        assert!(validate_pattern("http://localhost:3000").is_ok());
        assert!(validate_pattern("").is_err());
        assert!(validate_pattern("not-a-url").is_err());
        assert!(validate_pattern("https://api.*.com").is_err());
        assert!(validate_pattern("https://app.example.com/admin").is_err());
        assert!(validate_pattern(&"a".repeat(257)).is_err());
    }
}
