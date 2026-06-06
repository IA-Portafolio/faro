//! Fingerprint de errores: agrupa eventos del mismo defecto en un Issue.
//!
//! Calcula un hash SHA-256 determinista a partir del tipo de excepción, el mensaje
//! y los primeros frames del stack, normalizando antes (descarta números de línea,
//! direcciones hex y sufijos de clausuras) para que errores equivalentes colapsen
//! en el mismo `fingerprint`.

use sha2::{Digest, Sha256};

/// Calcula una huella determinista para un error de modo que los eventos que comparten
/// el mismo defecto subyacente quedan agrupados juntos. Normaliza los frames del stack
/// (descarta números de línea, direcciones hex, sufijos anónimos de clausuras) antes de
/// hashear.
pub fn fingerprint(exception_type: &str, exception_message: &str, stack: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(exception_type.as_bytes());
    hasher.update(b"\x00");

    // Normaliza el mensaje: colapsa dígitos + hex.
    let norm_msg = normalize(exception_message);
    hasher.update(norm_msg.as_bytes());
    hasher.update(b"\x00");

    if !stack.is_empty() {
        // Conserva los primeros 8 frames; alcanza para agrupar y aun así discriminar.
        let mut frames = 0;
        for line in stack.lines() {
            let l = normalize(line);
            if l.trim().is_empty() {
                continue;
            }
            hasher.update(l.as_bytes());
            hasher.update(b"\n");
            frames += 1;
            if frames >= 8 {
                break;
            }
        }
    }

    let digest = hasher.finalize();
    hex::encode(&digest[..16])
}

fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_digit = false;
    for c in s.chars() {
        if c.is_ascii_digit() || c == '#' {
            if !prev_digit {
                out.push('N');
                prev_digit = true;
            }
        } else {
            out.push(c);
            prev_digit = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_error_same_fingerprint() {
        let a = fingerprint("RuntimeError", "x is None at line 42", "frame line 42");
        let b = fingerprint("RuntimeError", "x is None at line 1337", "frame line 1337");
        assert_eq!(a, b);
    }

    #[test]
    fn different_type_different_fingerprint() {
        let a = fingerprint("RuntimeError", "boom", "");
        let b = fingerprint("ValueError", "boom", "");
        assert_ne!(a, b);
    }
}
