//! Contrato de versión entre el backend y los SDKs.
//!
//! Faro usa **un único entero de protocolo** que se incrementa solo cuando
//! cambia el shape del wire (payloads JSON, semánticas de headers, etc.) de
//! forma incompatible. Los SDKs declaran qué rango de protocolos soportan;
//! el backend declara su propio rango en `/healthz` y opcionalmente
//! rechaza ingestas fuera de rango.
//!
//! Ver `docs/adr/0008-sdk-version-compatibility.md` para el modelo de
//! compatibilidad completo.

use serde::Serialize;

/// Versión del backend tomada de `Cargo.toml`. Aparece en `/healthz`
/// solo como metadato (los SDKs no la usan para decidir nada).
pub const BACKEND_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Versión actual del protocolo wire. **Se incrementa SOLO con breaking
/// changes** del contrato (no con cambios menores o aditivos).
pub const PROTOCOL_CURRENT: u32 = 1;

/// Versión mínima del protocolo que el backend todavía acepta. Subirla
/// es un breaking change para SDKs viejos — comunicar via release notes.
pub const PROTOCOL_MIN_SUPPORTED: u32 = 1;

/// Versión máxima del protocolo que el backend reconoce. Hoy
/// coincide con `PROTOCOL_CURRENT`; en periodos de migración puede
/// ser mayor (acepta el nuevo + el viejo).
pub const PROTOCOL_MAX_SUPPORTED: u32 = 1;

/// Header HTTP que los SDKs envían en cada request de ingesta para
/// declarar qué versión de protocolo están hablando.
pub const HEADER_PROTOCOL: &str = "Faro-Protocol-Version";

/// Header opcional con el nombre del SDK (`node`, `python`, `go`, etc.).
/// Sirve solo para telemetría — qué SDKs y versiones están en uso real.
pub const HEADER_SDK_NAME: &str = "Faro-SDK-Name";

/// Header opcional con la versión del SDK que origina la request.
pub const HEADER_SDK_VERSION: &str = "Faro-SDK-Version";

/// Header con el que el backend responde indicando el resultado de la
/// validación de compatibilidad. Valores: `ok`, `deprecated`, `unsupported`.
pub const HEADER_COMPAT: &str = "Faro-Compat";

/// Payload devuelto por `/healthz` para que clientes y monitores
/// puedan verificar el estado y la versión del protocolo.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub protocol: ProtocolInfo,
}

#[derive(Debug, Serialize)]
pub struct ProtocolInfo {
    pub current: u32,
    pub min_supported: u32,
    pub max_supported: u32,
}

impl HealthResponse {
    pub const fn current() -> Self {
        Self {
            status: "ok",
            version: BACKEND_VERSION,
            protocol: ProtocolInfo {
                current: PROTOCOL_CURRENT,
                min_supported: PROTOCOL_MIN_SUPPORTED,
                max_supported: PROTOCOL_MAX_SUPPORTED,
            },
        }
    }
}

/// Resultado de comprobar el header `Faro-Protocol-Version` de un cliente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatStatus {
    /// Dentro del rango soportado.
    Ok,
    /// Por debajo del mínimo soportado — el SDK debe actualizarse pronto.
    /// Hoy lo aceptamos con warning; en el futuro será rechazo.
    Deprecated,
    /// Por encima del máximo soportado — el SDK habla un protocolo más
    /// nuevo que el que entiende este backend. Rechazo inmediato.
    Unsupported,
}

impl CompatStatus {
    pub fn header_value(self) -> &'static str {
        match self {
            CompatStatus::Ok => "ok",
            CompatStatus::Deprecated => "deprecated",
            CompatStatus::Unsupported => "unsupported",
        }
    }
}

/// Evalúa el header `Faro-Protocol-Version` de una request. Si el header
/// no viene, se asume `PROTOCOL_CURRENT` para no romper clientes que no
/// hayan migrado aún a declarar versión explícitamente.
pub fn classify_protocol(header_value: Option<&str>) -> CompatStatus {
    let v = match header_value.and_then(|s| s.trim().parse::<u32>().ok()) {
        Some(v) => v,
        None => return CompatStatus::Ok,
    };
    if v < PROTOCOL_MIN_SUPPORTED {
        CompatStatus::Deprecated
    } else if v > PROTOCOL_MAX_SUPPORTED {
        CompatStatus::Unsupported
    } else {
        CompatStatus::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_header_assumes_current() {
        assert_eq!(classify_protocol(None), CompatStatus::Ok);
    }

    #[test]
    fn current_is_ok() {
        assert_eq!(classify_protocol(Some("1")), CompatStatus::Ok);
    }

    #[test]
    fn below_min_is_deprecated() {
        // Forzamos un valor < MIN para validar la rama; cuando MIN=1
        // este test todavía pasa porque "0" < 1.
        assert_eq!(classify_protocol(Some("0")), CompatStatus::Deprecated);
    }

    #[test]
    fn above_max_is_unsupported() {
        assert_eq!(classify_protocol(Some("999")), CompatStatus::Unsupported);
    }

    #[test]
    fn garbage_assumes_current() {
        assert_eq!(classify_protocol(Some("not-a-number")), CompatStatus::Ok);
    }
}
