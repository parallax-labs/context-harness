# Data Platform Architecture

Our data platform enables analytics, machine learning, and real-time data processing at scale.

---

## Architecture Overview

```
Sources → Ingestion → Processing → Storage → Serving
                                                 ↓
                                           ML Training
                                                 ↓
                                           Model Serving
```

---

## Data Ingestion

### Batch Ingestion (dbt + Airflow)

Hourly and daily batch jobs extract data from operational databases into Snowflake:

```sql
-- dbt model: orders_enriched
SELECT
    o.id,
    o.created_at,
    o.total_amount_cents,
    o.status,
    u.segment,
    u.lifetime_value_cents,
    p.category,
    p.brand
FROM {{ ref('stg_orders') }} o
JOIN {{ ref('stg_users') }} u ON o.user_id = u.id
JOIN {{ ref('stg_products') }} p ON o.product_id = p.id
WHERE o.created_at >= '{{ var("start_date") }}'
```

### Stream Processing (Kafka + Flink)

Real-time events flow through Kafka to Apache Flink for:
- Fraud detection (sub-second decisions)
- Real-time inventory updates
- Live conversion funnel analytics
- Personalization signal aggregation

```java
DataStream<OrderEvent> orders = env
    .fromSource(kafkaSource, WatermarkStrategy.forMonotonousTimestamps(), "orders")
    .keyBy(OrderEvent::getUserId)
    .window(TumblingEventTimeWindows.of(Time.minutes(5)))
    .aggregate(new OrderAggregator());
```

---

## Data Warehouse (Snowflake)

### Schema Organization

| Schema | Purpose | Refresh |
|--------|---------|---------|
| `raw` | Unmodified source data | Continuous |
| `staging` | Cleaned, typed, deduplicated | Hourly |
| `marts` | Business-ready aggregations | Hourly |
| `ml_features` | Feature store for ML models | Daily |

### Data Contracts

Every table has a contract defined in YAML:

```yaml
model:
  name: orders_enriched
  description: "Orders with user and product dimensions"
  columns:
    - name: id
      type: string
      tests: [not_null, unique]
    - name: total_amount_cents
      type: integer
      tests: [not_null, positive]
    - name: created_at
      type: timestamp
      tests: [not_null]
  freshness:
    warn_after: 2 hours
    error_after: 4 hours
```

Breaking contract changes require:
1. ADR with justification
2. 2-week migration window
3. Downstream consumer notification
4. Backward-compatible transition period

---

## Machine Learning Infrastructure

### Feature Store

We use Feast for online and offline feature serving:

```python
from feast import FeatureStore

store = FeatureStore("feature_repo/")

# Online serving (low latency)
features = store.get_online_features(
    features=["user_features:lifetime_value", "user_features:order_count"],
    entity_rows=[{"user_id": "usr_123"}]
)

# Offline training (batch)
training_df = store.get_historical_features(
    entity_df=entity_df,
    features=["user_features:lifetime_value", "product_features:category"]
)
```

### Model Training Pipeline

1. **Data preparation:** dbt transforms → Feast features
2. **Training:** SageMaker training jobs with experiment tracking (MLflow)
3. **Evaluation:** Automated evaluation against holdout set
4. **Registry:** Model registered in MLflow with metrics and artifacts
5. **Deployment:** Canary deployment via SageMaker endpoints

### Model Serving

| Use Case | Serving Pattern | Latency Target |
|----------|----------------|----------------|
| Fraud detection | Real-time (gRPC) | < 50ms |
| Recommendations | Near-real-time (REST) | < 200ms |
| Forecasting | Batch (Airflow) | N/A |
| Search ranking | Real-time (REST) | < 100ms |

---

## Data Quality

### Automated Checks

- **Schema validation:** Great Expectations checks on every pipeline run
- **Freshness monitoring:** Alerts if data is stale beyond SLA
- **Volume anomaly detection:** Statistical checks for unexpected drops/spikes
- **Cross-source reconciliation:** Daily counts compared across systems

### Data Lineage

We track data lineage automatically via dbt and Airflow:

```
raw.stripe_charges
    → staging.stg_payments
        → marts.revenue_by_day
            → Dashboard: "Revenue Overview"
            → ML Model: "Churn Prediction"
```

Lineage helps answer: "If this source table changes, what dashboards and models are affected?"

---

## Access Control

| Role | Access | Approval |
|------|--------|----------|
| Analyst | Read marts schemas | Team lead |
| Data Engineer | Read/write all schemas | Data lead |
| ML Engineer | Read staging + ml_features | Data lead |
| Service Account | Specific tables via policy | Data lead + Security |

PII data requires additional approval from the Privacy team and is accessed via tokenized views only.

