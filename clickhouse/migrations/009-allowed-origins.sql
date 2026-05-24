-- Verificación de origen para el SDK browser/RUM. El token de ingesta es público
-- (viaja en el bundle JS), así que cualquiera que lo sniffe puede mandar eventos
-- desde fuera del dominio del cliente. La whitelist se aplica SOLO cuando el
-- request trae `Origin` (los SDKs server-side no lo mandan; ahí el bearer alcanza).
--
-- Formato JSON:
--   { "enabled": bool, "origins": ["https://app.example.com", "https://*.example.com"] }
--
-- Vacío o `enabled=false` → fail-open: cualquier Origin pasa (backward compat con
-- proyectos creados antes de esta feature, y opt-in explícito en los nuevos).
ALTER TABLE faro.projects
    ADD COLUMN IF NOT EXISTS allowed_origins String DEFAULT '';
