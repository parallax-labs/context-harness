# Architecture Decision Records

## ADR-001: Microservices over Monolith

**Status:** Accepted  
**Date:** 2025-06-15  
**Author:** Sarah Chen, Principal Engineer

### Context

Our monolithic application has grown to 2.3 million lines of code. Build times exceed 45 minutes. Teams are blocking each other on deployments. The blast radius of any change affects the entire system.

### Decision

We will decompose the monolith into domain-aligned microservices over the next 18 months. Each bounded context will become an independently deployable service.

### Consequences

- Teams can deploy independently with reduced coordination overhead
- We must invest in service mesh infrastructure (Istio)
- Cross-service transactions require saga patterns
- Operational complexity increases significantly — we need robust observability

---

## ADR-002: Event-Driven Architecture with Kafka

**Status:** Accepted  
**Date:** 2025-07-22  
**Author:** Marcus Rivera, Staff Engineer

### Context

Synchronous REST calls between services create tight coupling and cascade failures. The order processing pipeline requires seven service calls in sequence, creating a 3-second latency budget.

### Decision

Adopt Apache Kafka as the backbone for asynchronous inter-service communication. Domain events will be published to topic partitions. Services consume events and maintain local read models.

### Consequences

- Decoupled services improve resilience — one service failure doesn't cascade
- Eventual consistency requires careful handling in the UI layer
- We need schema registry (Confluent) for event contract management
- Engineers must understand event sourcing patterns

---

## ADR-003: PostgreSQL as Primary Datastore

**Status:** Accepted  
**Date:** 2025-08-10  
**Author:** Priya Patel, Database Engineering Lead

### Context

We evaluated PostgreSQL, CockroachDB, and DynamoDB for our primary datastore needs. Our workload is 70% reads, 30% writes with complex query patterns including full-text search and JSON operations.

### Decision

PostgreSQL 16 with logical replication for read replicas. We will use JSONB columns for flexible metadata and pg_trgm for fuzzy text search.

### Consequences

- Mature ecosystem with excellent tooling
- Logical replication enables zero-downtime migrations
- We must manage connection pooling carefully (PgBouncer)
- Vertical scaling limits require careful schema design and partitioning strategy

---

## ADR-004: Rust for Performance-Critical Services

**Status:** Accepted  
**Date:** 2025-09-01  
**Author:** Alex Kim, Platform Team Lead

### Context

Our data ingestion pipeline processes 50,000 events per second. The current Python implementation consumes 32GB of RAM and occasionally drops events during traffic spikes. Garbage collection pauses cause latency outliers at p99.

### Decision

Rewrite the ingestion pipeline and matching engine in Rust. Use Tokio for async I/O and zero-copy deserialization with serde.

### Consequences

- 10x throughput improvement with 4x less memory
- Compilation guarantees eliminate entire classes of runtime errors
- Smaller talent pool — must invest in Rust training
- Build times are longer than Go but the safety guarantees are worth it

