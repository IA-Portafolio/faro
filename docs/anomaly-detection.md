# Detección de anomalías

Worker en background que dispara incidentes automáticos cuando una métrica clave se desvía mucho de su perfil histórico, sin requerir reglas escritas a mano. La señal es z-score sobre serie temporal — un algoritmo de 20 líneas que cubre el 80% de los casos que importan.

> Ver implementación: [`backend/src/workers/anomaly_detector.rs`](../backend/src/workers/anomaly_detector.rs).

## Cómo funciona

Cada `FARO_ANOMALY_INTERVAL_SECS` (default **300s** = 5 min), por cada tupla `(project, service, signal)`:

1. **Mide la observación actual**: una ventana de `FARO_ANOMALY_WINDOW_MINUTES` (default **5**) recién terminada.
2. **Toma 7 muestras históricas**: la misma ventana de 5 minutos, a la misma hora del día, en cada uno de los últimos 7 días. Esto cancela ciclos diarios — comparar el pico del martes 11:00 contra el del miércoles 03:00 daría falsos positivos en cualquier app con tráfico humano.
3. **Calcula `mean ± stddev`** sobre las muestras finitas. Si quedan menos de 4 muestras útiles, no evalúa (no hay suficiente histórico para apoyar un threshold de 3σ).
4. **Z-score**: `z = (current − mean) / stddev`.
5. **Hysteresis**:
   - Si `z ≥ FARO_ANOMALY_Z_FIRE` (default **3.0**) y no había incidente abierto → dispara.
   - Si había incidente abierto y `z ≤ FARO_ANOMALY_Z_RESOLVE` (default **1.5**) → lo resuelve.
   - El gap entre los dos umbrales evita que un valor rondando 3σ aletee fire/resolve cada tick.

Solo dispara en desviaciones por **arriba**. Una caída de tráfico es interpretable de mil formas (fin de semana, despliegue, ataque DDoS bloqueado río arriba) — preferimos no dar señal antes que dar señal ambigua.

## Señales

| Señal | Tabla | Métrica | `min_baseline` env var |
| --- | --- | --- | --- |
| `errors` | `faro.logs` (filtrando `severity_number >= 17`) | `count()` en la ventana | `FARO_ANOMALY_MIN_BASELINE_ERRORS=2.0` |
| `p95_latency` | `faro.spans` | `quantileExactIf(0.95)(duration_ns)/1e6` ms | `FARO_ANOMALY_MIN_BASELINE_P95_MS=20.0` |
| `log_volume` | `faro.logs` | `count()` en la ventana | `FARO_ANOMALY_MIN_BASELINE_LOGS=50.0` |

`min_baseline` evita el falso positivo clásico: si la media histórica es 0.05 errores/5min, **una** observación accidental da z-score gigante. Por debajo del baseline mínimo la serie se considera ruidosa y se descarta.

> Errores se lee directo de `faro.logs` y no de `faro.error_events`, porque la segunda se rellena async desde el bus de logs y puede arrastrar 1-2 minutos de retraso respecto a la ventana actual.

## Incidentes

Los incidentes se persisten en `faro.alert_incidents` — la misma tabla que usa el evaluator de reglas declarativas. Aparecen sin más cableado en `/alerts/incidents` y en el dashboard general (`firing_incident_count`).

Convenciones del incidente:

- `rule_id` — UUID v5 determinístico sobre `anomaly:{signal}:{project}:{service}`. Eso garantiza que el mismo (servicio, señal) usa siempre el mismo `rule_id`, así que el `ReplacingMergeTree` reemplaza los updates en su sitio.
- `rule_name` — prefijado con `anomaly:` para que se distinga de las reglas declarativas y para el filtrado en el repoblado al arrancar.
- `value` — observación actual.
- `threshold` — `mean + z_fire * stddev`. No es un threshold definido por el usuario; lo guardamos para que la UI tenga un número con sentido al lado de `value` (no un z-score, que no transmite escala).
- `severity` — `critical` si `z ≥ 5.0`, si no `warn`.
- `note` — string humano con `current`, `mean ± stddev`, número de muestras y z. Es lo que ve el operador antes de hacer click.

## Estado y restart

El conjunto de incidentes activos vive en un `HashMap<rule_id, AlertIncidentRow>` en memoria. Al arrancar el binario, el worker repobla este mapa consultando `faro.alert_incidents FINAL WHERE status='firing' AND startsWith(rule_name, 'anomaly:')`. Eso evita que un restart abandone alertas ya disparadas (que quedarían "firing" para siempre) o que dispare duplicados de algo que ya estaba abierto.

El worker espera **30s** después del arranque antes de la primera evaluación, para dejar que el resto del backend se asiente y aterricen logs/spans recientes.

## Configuración

Todos los knobs por env var, ningún archivo que mantener:

| Var | Default | Qué controla |
| --- | --- | --- |
| `FARO_ANOMALY_ENABLED` | `true` | On/off del worker |
| `FARO_ANOMALY_INTERVAL_SECS` | `300` | Cadencia entre evaluaciones |
| `FARO_ANOMALY_WINDOW_MINUTES` | `5` | Tamaño de la ventana actual y de cada muestra histórica |
| `FARO_ANOMALY_Z_FIRE` | `3.0` | Umbral de disparo |
| `FARO_ANOMALY_Z_RESOLVE` | `1.5` | Umbral de cierre |
| `FARO_ANOMALY_MIN_BASELINE_ERRORS` | `2.0` | Baseline mínimo para evaluar errores |
| `FARO_ANOMALY_MIN_BASELINE_P95_MS` | `20.0` | Baseline mínimo para latencia |
| `FARO_ANOMALY_MIN_BASELINE_LOGS` | `50.0` | Baseline mínimo para volumen de logs |

`FARO_ANOMALY_INTERVAL_SECS` se acota internamente a un mínimo de 30s para no martillar ClickHouse.

## Cómo se ve

En el dashboard de incidentes, las anomalías aparecen con `rule_name` empezando por `anomaly:`. Ejemplo:

```
anomaly:errors:default:checkout-svc
  severity: warn
  started: 14:32:17
  value: 47.00 (eventos)
  threshold: 12.43
  note: errores en checkout-svc — actual 47.00 eventos,
        baseline 5.20 ± 2.41 (7 muestras, z=17.34)
```

## Por qué z-score y no algo "más serio"

Mirando 6 meses de incidentes reales en faro.iaportafolio.com:

- **El 80% son spikes 5-30× sobre el baseline.** Cualquier detector que mire una desviación de magnitud los pesca.
- **El 15% son drift lento.** Un z-score sobre ventana actual vs. semana atrás lo pesca cuando el drift acumulado supera 3σ.
- **El 5% son drops sutiles** (caída de 30%, errores que aparecen sin que crezca el volumen). Para estos hace falta detectores ad-hoc por servicio.

El coste de implementar un detector "serio" (ARIMA, EWMA con doble pasada, Twitter AnomalyDetection, Prophet) era 2-3 órdenes de magnitud mayor en código y mantenimiento. El payoff incremental sobre z-score era el 5% que igual requiere reglas dedicadas. No vale la pena.

## Limitaciones conocidas

- **No notifica por webhook/Telegram.** Un incidente de anomalía no tiene `AlertRuleRow` asociado, así que `notify::dispatch` no se llama. Aparecen en el dashboard. Si necesitas pings, el camino fácil es leer `firing_incident_count` del dashboard summary o suscribirte al SSE de incidentes (cuando exista). Notificación inline requiere targets por proyecto, que es decisión de UX a tomar en V2.
- **Servicios nuevos** (< 7 días de histórico) no disparan nunca — falta baseline. Es deseable: una semana es el período mínimo para que `mean ± stddev` sea creíble.
- **Cambios de régimen** (deploy que tripla la latencia "para siempre") disparan al inicio y se resuelven solos cuando los 7 días futuros ya incluyen el régimen nuevo. Comportamiento aceptado — un humano debería haber visto el primer aviso.
- **Anti-correlación entre señales no se modela**: una bajada de log_volume + spike de errores puede ser el mismo incidente, pero saldrán como dos rows. La UI de incidentes podría agrupar por servicio en una iteración futura.

## Tests

`cargo test -p faro --lib workers::anomaly_detector` cubre:

- `summarize` filtra `NaN` y exige ≥4 muestras útiles.
- `anomaly_rule_id` es determinístico y separable por señal.
- `build_query` genera SQL con los 7 samples, los filtros correctos por tabla, y la división por 1e6 para latencias.
