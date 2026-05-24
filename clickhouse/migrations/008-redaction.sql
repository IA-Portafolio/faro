-- PII redaction por proyecto. El campo es un blob JSON con:
--   { "enabled": bool,
--     "builtins": ["email","jwt","credit_card","bearer","password_kv","apikey_kv","ip"],
--     "custom": [ { "name":"...", "pattern":"...", "replacement":"[REDACTED]" }, ... ] }
-- El backend lo parsea, compila los regex una vez al cachearlo, y aplica
-- antes de mandar el row al canal de ingesta. Si el JSON está vacío o malformado,
-- redaction se considera deshabilitada para ese proyecto (fail-open hacia "no tocar
-- los datos", no hacia "tocar todo").
ALTER TABLE faro.projects
    ADD COLUMN IF NOT EXISTS redaction_rules String DEFAULT '';
