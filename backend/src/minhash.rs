//! MinHash signatures para clustering aproximado de errores.
//!
//! El [`fingerprint`](crate::fingerprint) actual es un hash exacto: dos
//! `NullPointerException` con frames distintos (versión del binario distinta,
//! posiciones de inlining diferentes, clases anónimas con sufijos cambiantes)
//! producen fingerprints distintos y aparecen como "issues" separados.
//! Operacionalmente son el mismo problema.
//!
//! MinHash resuelve esto produciendo una **firma** de K enteros sobre el conjunto
//! de "shingles" del texto del error. La similitud Jaccard estimada entre dos
//! errores = (# posiciones donde sus firmas coinciden) / K. Con K=128 y un umbral
//! Jaccard ≥ 0.85, agrupamos errores semánticamente iguales aunque su fingerprint
//! exacto difiera.
//!
//! Construcción evitando deps externas:
//! - Cada shingle se hashea con SHA-256; los primeros 8 bytes son `h1`, los
//!   siguientes 8 son `h2` (dos hashes independientes de calidad criptográfica).
//! - Las K permutaciones se simulan con hashing universal de Carter–Wegman:
//!   `h_i(x) = (a_i * h1 + b_i * h2) mod P` con P = 2^61 - 1 (Mersenne, cabe en
//!   u64 sin overflow después del wrapping_mul).
//! - Los `(a_i, b_i)` están fijos (seed determinístico) para que la firma de un
//!   mismo error sea byte-idéntica cruzando reinicios.
//!
//! Coste: por fingerprint nuevo el compactador hashea ~100-500 shingles con
//! SHA-256 (~500 ns c/u), luego K=128 productos por shingle. Total ~5-25 ms.
//! El worker corre cada 30 min — sobra.

use sha2::{Digest, Sha256};

/// Número de permutaciones / longitud de la firma. Cambiarlo invalida todas las
/// firmas ya persistidas en `faro.error_clusters` — no tocar sin migración.
pub const K: usize = 128;

/// Mersenne prime 2^61 - 1. Permite usar `wrapping_*` en u64 sin colisión y
/// produce buena distribución para hashing universal.
const PRIME: u64 = (1u64 << 61) - 1;

/// Firma MinHash de K=128 enteros.
pub type Signature = [u64; K];

/// Inicializa la firma con MAX (cualquier hash real será menor).
pub fn empty_signature() -> Signature {
    [u64::MAX; K]
}

/// Estima similitud Jaccard entre dos firmas. Resultado en [0.0, 1.0].
/// Si las firmas no tienen el mismo tamaño se devuelve 0.0 — caso no esperado
/// pero defensivo contra rows persistidas con K distinto en el futuro.
pub fn jaccard(a: &[u64], b: &[u64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let matches = a.iter().zip(b.iter()).filter(|(x, y)| x == y).count();
    matches as f64 / a.len() as f64
}

/// Calcula la firma MinHash de una lista de shingles. Pasarlos como `&str` evita
/// copias innecesarias.
pub fn signature(shingles: &[&str]) -> Signature {
    let mut sig = empty_signature();
    if shingles.is_empty() {
        return sig;
    }
    let perms = perm_constants();
    for s in shingles {
        let (h1, h2) = hash_pair(s.as_bytes());
        for i in 0..K {
            let (a, b) = perms[i];
            // (a * h1 + b * h2) mod PRIME, todo en wrapping para no panic.
            let hi = a.wrapping_mul(h1).wrapping_add(b.wrapping_mul(h2)) % PRIME;
            if hi < sig[i] {
                sig[i] = hi;
            }
        }
    }
    sig
}

/// Convierte un texto en shingles para MinHash:
///
/// 1. Normaliza (lowercase, colapsa dígitos a 'N', borra hex addresses tipo `0xAB12`,
///    elimina sufijos anónimos JVM/.NET (`$$Lambda$xxx`, `$xxx`) que rotan entre
///    builds y dañan la firma).
/// 2. Tokeniza por whitespace + puntuación.
/// 3. Devuelve trigramas de tokens contiguos (word-3-grams) — buenos para detectar
///    "mismo stack trace con diferencias de líneas y sufijos".
///
/// Para textos muy cortos (<3 tokens), cae a char-5-grams sobre el texto normalizado
/// para no devolver vacío.
pub fn shingle(text: &str) -> Vec<String> {
    let normalized = normalize(text);
    let tokens: Vec<&str> = normalized
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '.' | ','
                        | ';'
                        | ':'
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '<'
                        | '>'
                        | '/'
                        | '\\'
                        | '"'
                        | '\''
                )
        })
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.len() >= 3 {
        let mut out = Vec::with_capacity(tokens.len().saturating_sub(2));
        for w in tokens.windows(3) {
            out.push(format!("{} {} {}", w[0], w[1], w[2]));
        }
        return out;
    }

    // Fallback char-5-grams sobre el texto normalizado (sin espacios extras).
    let compact: String = normalized.chars().filter(|c| !c.is_whitespace()).collect();
    let chars: Vec<char> = compact.chars().collect();
    if chars.len() < 5 {
        // Texto trivial — devolver el normalizado entero como único shingle es lo
        // único razonable. La firma resultante es la del único shingle.
        return vec![compact];
    }
    let mut out = Vec::with_capacity(chars.len() - 4);
    for w in chars.windows(5) {
        out.push(w.iter().collect::<String>());
    }
    out
}

fn normalize(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut iter = lower.chars().peekable();
    let mut prev_digit = false;
    while let Some(c) = iter.next() {
        // Quita addresses hex `0x[0-9a-f]+`.
        if c == '0' {
            if let Some(&'x') = iter.peek() {
                iter.next();
                while iter.peek().map_or(false, |c| c.is_ascii_hexdigit()) {
                    iter.next();
                }
                out.push_str("0xN");
                prev_digit = false;
                continue;
            }
        }
        // Sufijos anónimos JVM/.NET tipo `$Lambda$123` o `$$anonfun$1`.
        if c == '$' {
            // Salta `$+` y luego identifier+digit sufijos.
            while iter.peek() == Some(&'$') {
                iter.next();
            }
            // Consume hasta el siguiente separador no-identificador.
            while iter.peek().map_or(false, |c| {
                c.is_ascii_alphanumeric() || *c == '_' || *c == '/'
            }) {
                iter.next();
            }
            out.push_str("$X");
            prev_digit = false;
            continue;
        }
        if c.is_ascii_digit() {
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

fn hash_pair(bytes: &[u8]) -> (u64, u64) {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let h1 = u64::from_le_bytes(digest[0..8].try_into().expect("digest len"));
    let h2 = u64::from_le_bytes(digest[8..16].try_into().expect("digest len"));
    (h1, h2)
}

/// Constantes (a_i, b_i) precomputadas con un PRNG seedeado determinísticamente.
/// Se calculan **una sola vez** por proceso vía OnceLock y luego son referencia
/// inmutable; el coste por llamada a `signature()` es 0 amortizado.
fn perm_constants() -> &'static [(u64, u64); K] {
    use std::sync::OnceLock;
    static CONSTS: OnceLock<[(u64, u64); K]> = OnceLock::new();
    CONSTS.get_or_init(|| {
        // PRNG simple Linear Congruential Generator. Seed fijo — no hace falta
        // criptografía; sólo necesitamos uniformidad razonable y determinismo
        // cross-restart para que las firmas viejas sigan comparándose con las nuevas.
        let mut state: u64 = 0xfa_e0_31_72_88_53_22_11;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            state
        };
        let mut arr = [(0u64, 0u64); K];
        for entry in arr.iter_mut() {
            // a en [1, PRIME-1], b en [0, PRIME-1] — `mod PRIME` mapea uniforme.
            let a = (next() % (PRIME - 1)) + 1;
            let b = next() % PRIME;
            *entry = (a, b);
        }
        arr
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_jaccard_one() {
        let s = "NullPointerException at com.example.Foo.bar line 42";
        let sig_a = signature(&shingle(s).iter().map(String::as_str).collect::<Vec<_>>());
        let sig_b = signature(&shingle(s).iter().map(String::as_str).collect::<Vec<_>>());
        assert!((jaccard(&sig_a, &sig_b) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn similar_errors_have_high_jaccard() {
        // Mismo error semántico: NPE en el mismo método, distinto número de línea
        // y un sufijo anónimo distinto (típico tras un rebuild).
        let a = "NullPointerException at com.example.Foo$$Lambda$123.bar:42 frame at line 100";
        let b = "NullPointerException at com.example.Foo$$Lambda$987.bar:88 frame at line 200";
        let sa = signature(&shingle(a).iter().map(String::as_str).collect::<Vec<_>>());
        let sb = signature(&shingle(b).iter().map(String::as_str).collect::<Vec<_>>());
        let j = jaccard(&sa, &sb);
        assert!(j > 0.8, "Jaccard demasiado bajo: {j}");
    }

    #[test]
    fn unrelated_errors_have_low_jaccard() {
        let a = "NullPointerException at com.example.Foo.bar:42";
        let b = "ConnectionRefused upstream postgres:5432 timeout after 30s";
        let sa = signature(&shingle(a).iter().map(String::as_str).collect::<Vec<_>>());
        let sb = signature(&shingle(b).iter().map(String::as_str).collect::<Vec<_>>());
        let j = jaccard(&sa, &sb);
        assert!(j < 0.2, "Jaccard demasiado alto: {j}");
    }

    #[test]
    fn signature_is_deterministic_across_calls() {
        let s = "some error message here with multiple words";
        let shingles = shingle(s);
        let refs: Vec<&str> = shingles.iter().map(String::as_str).collect();
        let a = signature(&refs);
        let b = signature(&refs);
        assert_eq!(a, b);
    }

    #[test]
    fn shingle_handles_short_text() {
        let s = "hi";
        let sh = shingle(s);
        assert!(
            !sh.is_empty(),
            "shingle no debería devolver vacío para texto corto"
        );
    }

    #[test]
    fn normalize_collapses_digits_and_hex_and_anon() {
        let n = normalize("Frame at 0xdeadbeef.method$$Lambda$123.line:4567");
        // Sin números explícitos, sin hex address, sin sufijo anónimo identificable.
        // Marker `0xN` mayúscula igual que el `N` que reemplaza dígitos genéricos.
        assert!(!n.contains("4567"), "no colapsó dígitos: {n}");
        assert!(!n.contains("deadbeef"), "no removió hex addr: {n}");
        assert!(!n.contains("lambda$123"), "no removió sufijo anónimo: {n}");
        assert!(n.contains("0xN"), "marcador de hex addr ausente: {n}");
    }
}
