//! Validación de las queries SQL crudas de las reglas de alerta.
//!
//! El `query` de una `alert_rule` es texto libre que el usuario escribe y que el
//! evaluador ejecuta como `SELECT toFloat64({query}) AS value` contra ClickHouse
//! con el usuario `faro` (RW, sin `readonly`). Sin validación, una regla con una
//! table-function de red/fichero —`url('http://169.254.169.254/latest/meta-data/')`,
//! `file('/etc/passwd')`, `s3(...)`, `remote(...)`, `executable(...)`— convierte al
//! backend en un primitivo de SSRF / lectura de ficheros / ejecución de comandos,
//! disparado cada `interval_seconds` con la identidad de red del backend de prod.
//!
//! Esta validación es la **primera línea**: denylist de table-functions peligrosas
//! + rechazo de multi-statement (`;`) y comentarios (que podrían romper el wrapper
//! `) AS value`). NO es un sandbox completo — el hardening definitivo es un usuario
//! ClickHouse `readonly` dedicado para el evaluador (perfil en `users.d` con
//! `readonly=2` y las table-functions de red deshabilitadas). Se aplica al
//! crear/editar la regla (`api::alerts`) Y en el evaluador
//! (`workers::alert_evaluator`), defensa en profundidad por si quedaran reglas
//! viejas persistidas antes de este cambio.

/// Table-functions de ClickHouse que permiten acceso a red, ficheros locales,
/// almacenes externos o ejecución de procesos. Cualquiera de ellas en una query
/// de alerta es un vector de SSRF / exfiltración / RCE y se rechaza.
const FORBIDDEN_TABLE_FUNCTIONS: &[&str] = &[
    "url",
    "urlcluster",
    "file",
    "s3",
    "s3cluster",
    "s3queue",
    "gcs",
    "remote",
    "remotesecure",
    "cluster",
    "clusterallreplicas",
    "hdfs",
    "hdfscluster",
    "jdbc",
    "odbc",
    "mysql",
    "postgresql",
    "mongodb",
    "redis",
    "sqlite",
    "azureblobstorage",
    "azureblobstoragecluster",
    "deltalake",
    "hudi",
    "iceberg",
    "executable",
];

/// Valida una query de regla de alerta. `Ok(())` si es segura para ejecutar como
/// `SELECT toFloat64({query}) AS value`; `Err(motivo)` si debe rechazarse.
pub fn validate_alert_query(query: &str) -> Result<(), &'static str> {
    let q = query.trim();
    if q.is_empty() {
        return Err("la query de la alerta no puede estar vacía");
    }
    // Multi-statement: un `;` permite romper el wrapper y ejecutar SQL arbitrario.
    if q.contains(';') {
        return Err("la query no puede contener ';' (multi-statement no permitido)");
    }
    // Comentarios: `--` o `/* */` pueden esconder payloads o cortar el `) AS value`.
    if q.contains("--") || q.contains("/*") || q.contains("*/") {
        return Err("la query no puede contener comentarios SQL ('--' o '/* */')");
    }
    // Denylist de table-functions de red/fichero/externas. Matcheamos el nombre
    // como llamada a función (`nombre(`) sobre una copia en minúsculas.
    let lower = q.to_ascii_lowercase();
    for &func in FORBIDDEN_TABLE_FUNCTIONS {
        if contains_function_call(&lower, func) {
            return Err("la query referencia una table-function prohibida \
                 (url/file/s3/remote/jdbc/executable/...) — bloqueada por seguridad");
        }
    }
    Ok(())
}

/// ¿Aparece `name` como llamada a función en `haystack` (ya en minúsculas)? Exige
/// que `name` esté delimitado a la izquierda por inicio-de-string o un byte que no
/// sea de identificador, y seguido (saltando espacios) de `(`. Así `url(` y `URL (`
/// matchean, pero `urls`, `my_file`, `fileSizeInBytes(` no (evita falsos positivos).
fn contains_function_call(haystack: &str, name: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(name) {
        let start = from + rel;
        let end = start + name.len();
        let left_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let mut j = end;
        while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r') {
            j += 1;
        }
        let right_ok = j < bytes.len() && bytes[j] == b'(';
        if left_ok && right_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::validate_alert_query;

    #[test]
    fn accepts_legit_aggregation_queries() {
        // Las reglas reales del README: countIf, quantile, sum sobre tablas faro.
        assert!(validate_alert_query(
            "(SELECT countIf(severity_number >= 17) FROM faro.logs WHERE timestamp > now() - INTERVAL :window_seconds SECOND)"
        )
        .is_ok());
        assert!(validate_alert_query(
            "(SELECT toFloat64(quantile(0.95)(duration_ns))/1e6 FROM faro.spans WHERE service_name='api')"
        )
        .is_ok());
        assert!(validate_alert_query(
            "(SELECT sum(success)/count()*100 FROM faro.monitor_results WHERE monitor_id = 'x')"
        )
        .is_ok());
    }

    #[test]
    fn rejects_ssrf_via_url_table_function() {
        let q = "(SELECT count() FROM url('http://169.254.169.254/latest/meta-data/', JSONEachRow, 'x UInt8'))";
        assert!(validate_alert_query(q).is_err());
        // Variantes de casing/espacios.
        assert!(validate_alert_query("SELECT 1 FROM URL ('http://x')").is_err());
    }

    #[test]
    fn rejects_file_read_and_other_external_functions() {
        assert!(validate_alert_query(
            "(SELECT * FROM file('/etc/passwd', LineAsString, 's String'))"
        )
        .is_err());
        assert!(validate_alert_query("SELECT 1 FROM s3('http://x/y', 'CSV')").is_err());
        assert!(validate_alert_query("SELECT 1 FROM remote('1.2.3.4:9000', system.one)").is_err());
        assert!(validate_alert_query(
            "SELECT 1 FROM executable('cat /etc/shadow', TSV, 's String')"
        )
        .is_err());
        assert!(
            validate_alert_query("SELECT 1 FROM postgresql('h:5432','db','t','u','p')").is_err()
        );
    }

    #[test]
    fn rejects_multistatement_and_comments() {
        assert!(validate_alert_query("SELECT 1; DROP TABLE faro.logs").is_err());
        assert!(validate_alert_query("SELECT 1 /* hidden */ FROM url('http://x')").is_err());
        assert!(validate_alert_query("SELECT 1 -- ) AS value\n FROM url('http://x')").is_err());
        assert!(validate_alert_query("   ").is_err());
    }

    #[test]
    fn does_not_false_positive_on_similar_identifiers() {
        // `urls`, `file_count`, `s3_bytes` como columnas/aliases no son table-functions.
        assert!(validate_alert_query("(SELECT count() FROM faro.logs WHERE urls > 0)").is_ok());
        assert!(validate_alert_query("(SELECT sum(file_count) FROM faro.metrics)").is_ok());
        assert!(validate_alert_query("(SELECT avg(fileSizeInBytes) FROM faro.logs)").is_ok());
    }
}
