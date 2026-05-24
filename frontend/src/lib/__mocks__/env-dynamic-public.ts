// Stub de `$env/dynamic/public` para tests unitarios. En tests no hay variables
// públicas inyectadas; api.ts cae al fallback (http://localhost:8080) — y como
// los tests no hacen fetch, da igual.
export const env: Record<string, string | undefined> = {};
