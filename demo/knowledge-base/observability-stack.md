# Observability Stack

Our observability strategy follows the three pillars: metrics, logs, and traces.

---

## Metrics (Datadog)

### Key Dashboards

| Dashboard | Purpose | Alert Threshold |
|-----------|---------|-----------------|
| Service Health | Request rate, error rate, latency | Error rate > 1%, p99 > 2s |
| Infrastructure | CPU, memory, disk, network | CPU > 80%, memory > 85% |
| Business KPIs | Orders/min, revenue, conversion | Orders drop > 20% |
| Kafka | Consumer lag, partition balance | Lag > 10,000 messages |
| Database | Connections, query latency, replication lag | Connections > 80%, lag > 1s |

### Custom Metrics Convention

```
acme.<service>.<subsystem>.<metric>
```

Examples:
- `acme.order_service.api.request_duration`
- `acme.order_service.kafka.messages_processed`
- `acme.order_service.db.query_duration`
- `acme.order_service.cache.hit_ratio`

### SLOs and Error Budgets

| Service | Availability SLO | Latency SLO (p99) | Error Budget/Month |
|---------|-------------------|--------------------|--------------------|
| Order API | 99.95% | 500ms | 21.9 minutes |
| Payment API | 99.99% | 200ms | 4.3 minutes |
| Search API | 99.9% | 1000ms | 43.8 minutes |
| User API | 99.95% | 300ms | 21.9 minutes |

When error budget is exhausted, the team must prioritize reliability work over feature development until the budget is replenished.

---

## Logging (Datadog Logs)

### Log Levels

| Level | Usage | Sampled in Prod |
|-------|-------|-----------------|
| ERROR | Unexpected failures requiring investigation | 100% |
| WARN | Degraded behavior, retryable errors | 100% |
| INFO | Key business events, request lifecycle | 10% |
| DEBUG | Detailed diagnostic information | 0% (enable via feature flag) |

### Structured Logging Format

All services must use structured JSON logging:

```json
{
  "timestamp": "2025-10-15T14:30:00.123Z",
  "level": "ERROR",
  "service": "order-service",
  "trace_id": "abc123",
  "span_id": "def456",
  "message": "Failed to process order",
  "error": {
    "type": "PaymentDeclined",
    "message": "Card expired",
    "stack_trace": "..."
  },
  "context": {
    "order_id": "ord_789",
    "user_id": "usr_012",
    "amount_cents": 5999
  }
}
```

### Rust Logging Setup

```rust
use tracing::{info, error, instrument};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[instrument(skip(db_pool))]
async fn process_order(order_id: &str, db_pool: &PgPool) -> Result<()> {
    info!(order_id, "Processing order");
    
    match do_payment(order_id).await {
        Ok(payment) => {
            info!(order_id, payment_id = %payment.id, "Payment succeeded");
            Ok(())
        }
        Err(e) => {
            error!(order_id, error = %e, "Payment failed");
            Err(e)
        }
    }
}
```

---

## Distributed Tracing (Datadog APM)

### Trace Propagation

All services must propagate trace context via HTTP headers:

```
traceparent: 00-abc123-def456-01
tracestate: dd=t.dm:-1
```

### Key Traces to Monitor

1. **Order lifecycle:** API → Validation → Payment → Fulfillment → Notification
2. **Search pipeline:** Query → Parse → Execute → Rank → Return
3. **Data sync:** Connector → Ingest → Transform → Store → Index

### Trace Sampling

| Environment | Sampling Rate |
|-------------|---------------|
| Development | 100% |
| Staging | 100% |
| Production | 10% base, 100% for errors, 100% for slow requests (>1s) |

---

## Alerting

### Alert Routing

```yaml
# PagerDuty escalation policy
- name: "P1 Critical"
  rules:
    - notify: on-call-primary
      delay: 0 minutes
    - notify: on-call-secondary
      delay: 5 minutes
    - notify: engineering-manager
      delay: 15 minutes

- name: "P2 Warning"
  rules:
    - notify: team-slack-channel
      delay: 0 minutes
    - notify: on-call-primary
      delay: 30 minutes
```

### Alert Hygiene

- Every alert must have a runbook link
- Alert fatigue review: monthly audit of alert frequency
- Goal: < 5 actionable pages per on-call shift
- Noisy alerts are either fixed, tuned, or deleted — never ignored

---

## Runbook Integration

Every alert links to a runbook in our engineering handbook. Runbooks must include:

1. **What this alert means** — plain English description
2. **Impact** — what users experience
3. **Investigation steps** — ordered diagnostic steps
4. **Mitigation** — immediate actions to reduce impact
5. **Resolution** — steps to fix the root cause
6. **Escalation** — who to page if you can't resolve

