-- Añade `default_project` y `default_time_range` a las preferencias de usuario.
-- Idempotente: el `IF NOT EXISTS` en cada ADD COLUMN evita el reintento ruidoso
-- en instancias que ya fueron migradas. Las filas existentes adquieren los
-- defaults '' y '1h', equivalente al comportamiento histórico (ningún
-- proyecto fijado, rango "última hora").

ALTER TABLE faro.user_preferences
    ADD COLUMN IF NOT EXISTS default_project    String                 DEFAULT '',
    ADD COLUMN IF NOT EXISTS default_time_range LowCardinality(String) DEFAULT '1h';
