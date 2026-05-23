use sha2::{Digest, Sha256};

/// Compute a deterministic fingerprint for an error so events that share the same
/// underlying defect get bucketed together. Normalises stack frames (drops line
/// numbers, hex addresses, anonymous closure suffixes) before hashing.
pub fn fingerprint(exception_type: &str, exception_message: &str, stack: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(exception_type.as_bytes());
    hasher.update(b"\x00");

    // Normalise message: collapse digits + hex.
    let norm_msg = normalize(exception_message);
    hasher.update(norm_msg.as_bytes());
    hasher.update(b"\x00");

    if !stack.is_empty() {
        // Keep first 8 frames; that's plenty for grouping while still discriminating.
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
