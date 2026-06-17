//! Test de la guarda anti-producción del fixture de integración.
//!
//! `require_test_clickhouse_url` impide que un `cargo test` accidental contra el
//! default `localhost:8123` (= ClickHouse de PRODUCCIÓN en el host de deploy)
//! escriba datos de test en prod. Ya pasó una vez (142 proyectos `test-*` + 44
//! usuarios `@test.local` que hubo que borrar a mano). Probamos la fn pura, sin
//! tocar el entorno real ni ClickHouse.

mod common;

use common::require_test_clickhouse_url;

#[test]
fn returns_url_when_explicitly_set() {
    let url = require_test_clickhouse_url(Ok("http://localhost:18123".to_string()));
    assert_eq!(url, "http://localhost:18123");
}

#[test]
#[should_panic(expected = "CLICKHOUSE_URL no está seteado")]
fn panics_when_var_absent() {
    // Var ausente → debe paniquear con la guía ANTES de conectar a ClickHouse,
    // de modo que un `cargo test` sin CLICKHOUSE_URL nunca toque prod.
    let _ = require_test_clickhouse_url(Err(std::env::VarError::NotPresent));
}
