//! Token bucket por `project_id` para la ingesta.
//!
//! Sin esto, un cliente buggeado que repite un loop con miles de records/s
//! degrada a todos los demás proyectos porque comparten el writer ClickHouse
//! río abajo (canales `IngestChannels` + worker único). El limiter se aplica
//! a los tres endpoints (OTLP/HTTP, OTLP/gRPC, `/logs` simple) compartiendo
//! el mismo bucket por proyecto — de lo contrario un cliente esquiva el límite
//! saltando de transporte.
//!
//! In-memory por proceso vía `governor` (GCRA). Faro corre un solo backend por
//! instancia productiva, así que la coordinación distribuida no aplica todavía;
//! si alguna vez hay más de un nodo, esto migra a Redis preservando la firma
//! [`IngestLimiter::check`].

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use governor::{
    clock::{Clock, DefaultClock},
    state::keyed::DefaultKeyedStateStore,
    InsufficientCapacity, Quota, RateLimiter,
};

type Inner = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

pub struct IngestLimiter {
    inner: Arc<Inner>,
    clock: DefaultClock,
}

#[derive(Debug, Clone, Copy)]
pub enum LimitOutcome {
    Allowed,
    /// El proyecto excedió su cuota. `retry_after` es el tiempo mínimo a esperar
    /// para que vuelvan a quedar tokens libres.
    Throttled {
        retry_after: Duration,
    },
    /// El batch pedido es mayor que la capacidad burst y no cabe nunca tal cual.
    /// Lo tratamos como rechazo con `Retry-After: 1` — el cliente debería partir
    /// el batch. Defendemos el sistema sin tirar 4xx sin pista de qué hacer.
    BatchTooLarge,
}

impl LimitOutcome {
    /// Segundos a esperar antes de reintentar. Siempre >= 1 cuando bloqueamos,
    /// para no devolver `Retry-After: 0` que algunos clientes interpretan como
    /// "reintenta ya" y entran en spinloop.
    pub fn retry_after_secs(&self) -> u64 {
        match self {
            LimitOutcome::Allowed => 0,
            LimitOutcome::Throttled { retry_after } => retry_after.as_secs().max(1),
            LimitOutcome::BatchTooLarge => 1,
        }
    }
}

impl IngestLimiter {
    /// `records_per_second` es la tasa sostenida por proyecto. Burst arranca en
    /// 2× esa tasa para absorber picos de batches grandes sin penalizar a un
    /// cliente que esté cumpliendo el promedio.
    pub fn new(records_per_second: u32) -> Self {
        let rps = NonZeroU32::new(records_per_second.max(1)).expect("rps >= 1");
        let burst = NonZeroU32::new(records_per_second.saturating_mul(2).max(rps.get()))
            .expect("burst >= 1");
        let quota = Quota::per_second(rps).allow_burst(burst);
        Self {
            inner: Arc::new(RateLimiter::keyed(quota)),
            clock: DefaultClock::default(),
        }
    }

    /// Reserva `n` tokens para `project_id`. No bloquea: si no hay tokens
    /// libres devuelve [`LimitOutcome::Throttled`] inmediatamente.
    pub fn check(&self, project_id: &str, n: u32) -> LimitOutcome {
        let Some(n_nz) = NonZeroU32::new(n) else {
            return LimitOutcome::Allowed;
        };
        match self.inner.check_key_n(&project_id.to_string(), n_nz) {
            Ok(Ok(_)) => LimitOutcome::Allowed,
            Ok(Err(not_until)) => {
                let wait = not_until.wait_time_from(self.clock.now());
                LimitOutcome::Throttled { retry_after: wait }
            }
            Err(InsufficientCapacity(_)) => LimitOutcome::BatchTooLarge,
        }
    }
}
