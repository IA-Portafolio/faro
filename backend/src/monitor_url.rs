//! Validación de URLs para monitores HTTP sintéticos.
//!
//! Sin validación, un usuario autenticado puede apuntar un monitor a
//! `http://169.254.169.254/latest/meta-data/` (credenciales cloud) o a servicios
//! de la red interna (`http://clickhouse:8123`, `http://localhost:…`); el worker
//! hace el request con la identidad de red del backend y expone la respuesta como
//! uptime/latencia/error_message. Este módulo es la primera línea de defensa,
//! análogo a la denylist de `alert_query.rs` para el primitivo de query.
//!
//! Se aplica al crear/editar el monitor (`api::monitors`) Y en el worker
//! (`workers::monitor_runner`) como defensa en profundidad por si quedan filas
//! anteriores a este cambio en la tabla.

use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// Valida que `raw` sea una URL segura para un monitor HTTP sintético.
///
/// Rechaza:
/// * Esquemas distintos de `http` / `https`.
/// * IPs privadas / link-local / reservadas: 127/8, 10/8, 172.16/12,
///   192.168/16, 169.254/16 (AWS metadata), 100.64/10 (RFC 6598), 0/8,
///   255.255.255.255; IPv6 loopback, ULA (fc00::/7), link-local (fe80::/10).
/// * `localhost` y cualquier nombre de host sin punto (nombres internos como
///   `clickhouse`, `redis`, `postgres` no tienen dominio cualificado).
pub fn validate_monitor_url(raw: &str) -> Result<(), String> {
    let url = Url::parse(raw).map_err(|e| format!("URL de monitor inválida: {e}"))?;

    match url.scheme() {
        "http" | "https" => {}
        s => {
            return Err(format!(
                "esquema '{s}' no permitido en monitores; solo http/https"
            ))
        }
    }

    let host = url
        .host()
        .ok_or_else(|| "la URL del monitor no tiene host".to_string())?;

    match host {
        Host::Ipv4(addr) => {
            if is_private_v4(addr) {
                return Err(format!(
                    "host '{addr}' es una dirección IP privada/reservada; \
                     no se puede usar como destino de monitor"
                ));
            }
        }
        Host::Ipv6(addr) => {
            if is_private_v6(addr) {
                return Err(format!(
                    "host '{addr}' es una dirección IPv6 privada/reservada; \
                     no se puede usar como destino de monitor"
                ));
            }
        }
        Host::Domain(name) => {
            let lower = name.to_ascii_lowercase();
            if lower == "localhost" || !lower.contains('.') {
                return Err(format!(
                    "host '{name}' no está permitido; los monitores solo pueden \
                     apuntar a hostnames públicos con dominio cualificado"
                ));
            }
        }
    }

    Ok(())
}

fn is_private_v4(addr: Ipv4Addr) -> bool {
    let [a, b, c, d] = addr.octets();
    // Loopback: 127.0.0.0/8
    a == 127
    // Unspecified: 0.0.0.0/8
    || a == 0
    // Private: 10.0.0.0/8
    || a == 10
    // Private: 172.16.0.0/12
    || (a == 172 && (16..=31).contains(&b))
    // Private: 192.168.0.0/16
    || (a == 192 && b == 168)
    // Link-local / AWS metadata: 169.254.0.0/16
    || (a == 169 && b == 254)
    // Shared address space: 100.64.0.0/10 (RFC 6598, CGNAT)
    || (a == 100 && (64..=127).contains(&b))
    // Broadcast
    || [a, b, c, d] == [255, 255, 255, 255]
}

fn is_private_v6(addr: Ipv6Addr) -> bool {
    let segs = addr.segments();
    addr.is_loopback()
        || addr.is_unspecified()
        // ULA: fc00::/7 (includes fd00::/8)
        || (segs[0] & 0xfe00) == 0xfc00
        // Link-local: fe80::/10
        || (segs[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_public_urls() {
        assert!(validate_monitor_url("https://example.com/health").is_ok());
        assert!(validate_monitor_url("http://api.example.org/ping").is_ok());
        // Public IP (8.8.8.8 is not private)
        assert!(validate_monitor_url("https://8.8.8.8/check").is_ok());
        // Port is fine
        assert!(validate_monitor_url("https://example.com:8443/api").is_ok());
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(validate_monitor_url("ftp://example.com").is_err());
        assert!(validate_monitor_url("file:///etc/passwd").is_err());
        assert!(validate_monitor_url("gopher://example.com").is_err());
        assert!(validate_monitor_url("data:text/html,<h1>x</h1>").is_err());
    }

    #[test]
    fn rejects_localhost_and_bare_hostnames() {
        assert!(validate_monitor_url("http://localhost/health").is_err());
        assert!(validate_monitor_url("http://localhost:8123/").is_err());
        assert!(validate_monitor_url("http://clickhouse/ping").is_err());
        assert!(validate_monitor_url("http://redis/status").is_err());
        assert!(validate_monitor_url("http://postgres/").is_err());
    }

    #[test]
    fn rejects_aws_metadata_and_private_ipv4() {
        // AWS/GCP metadata endpoint
        assert!(validate_monitor_url("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(validate_monitor_url("http://169.254.0.1/").is_err());
        // Loopback
        assert!(validate_monitor_url("http://127.0.0.1:8123/").is_err());
        assert!(validate_monitor_url("http://127.1.2.3/").is_err());
        // RFC 1918
        assert!(validate_monitor_url("http://10.0.0.1/api").is_err());
        assert!(validate_monitor_url("http://192.168.1.1/").is_err());
        assert!(validate_monitor_url("http://172.16.0.1/").is_err());
        assert!(validate_monitor_url("http://172.31.255.255/").is_err());
        // Shared address space (RFC 6598)
        assert!(validate_monitor_url("http://100.64.0.1/").is_err());
        assert!(validate_monitor_url("http://100.127.255.255/").is_err());
    }

    #[test]
    fn rejects_private_ipv6() {
        assert!(validate_monitor_url("http://[::1]/").is_err());
        assert!(validate_monitor_url("http://[fd12:3456::1]/").is_err());
        assert!(validate_monitor_url("http://[fc00::1]/").is_err());
        assert!(validate_monitor_url("http://[fe80::1]/").is_err());
    }

    #[test]
    fn rejects_invalid_url() {
        assert!(validate_monitor_url("not a url").is_err());
        assert!(validate_monitor_url("").is_err());
        assert!(validate_monitor_url("http://").is_err());
    }

    #[test]
    fn boundary_172_range() {
        // 172.15.x is NOT private (below .16)
        assert!(validate_monitor_url("http://172.15.0.1/").is_ok());
        // 172.32.x is NOT private (above .31)
        assert!(validate_monitor_url("http://172.32.0.1/").is_ok());
        // 172.16 and 172.31 are private
        assert!(validate_monitor_url("http://172.16.0.1/").is_err());
        assert!(validate_monitor_url("http://172.31.0.1/").is_err());
    }
}
