# Post-Mortem: Order Processing Outage — October 3, 2025

**Severity:** SEV-1  
**Duration:** 47 minutes  
**Incident Commander:** Marcus Rivera  
**Author:** Sarah Chen  

---

## Summary

On October 3, 2025 at 14:23 UTC, the order processing pipeline stopped processing new orders for 47 minutes. During this window, approximately 12,400 orders were queued but not processed, affecting 8,200 unique customers. No orders were lost — all queued orders were processed after recovery.

Revenue impact was estimated at $340,000 in delayed transactions (all recovered).

---

## Timeline

| Time (UTC) | Event |
|------------|-------|
| 14:23 | Kafka consumer lag alert fires for order-processor consumer group |
| 14:25 | On-call SRE (Marcus) acknowledges alert |
| 14:28 | Marcus identifies that all order-processor pods are in CrashLoopBackOff |
| 14:30 | SEV-1 declared, incident channel created |
| 14:32 | Application logs show: `panicked at 'Failed to deserialize OrderEvent: missing field "shipping_method"'` |
| 14:35 | Root cause identified: a deployment to the Order API 20 minutes earlier added a required `shipping_method` field to OrderEvent |
| 14:38 | Two options identified: (1) rollback Order API, (2) deploy fix to order-processor |
| 14:42 | Decision: rollback Order API as it's faster |
| 14:45 | Order API rolled back to previous version |
| 14:50 | Order-processor pods restart and begin draining the backlog |
| 15:10 | All queued orders processed, consumer lag returns to zero |
| 15:15 | SEV-1 resolved, incident channel archived |

---

## Root Cause Analysis

### 5 Whys

1. **Why did the order-processor crash?**  
   Because it couldn't deserialize the OrderEvent — the `shipping_method` field was missing.

2. **Why was the field missing?**  
   Because the Order API was deployed with a schema change that added `shipping_method` as a required field in OrderEvent, but the order-processor hadn't been updated to handle it.

3. **Why wasn't the order-processor updated first?**  
   Because the schema change wasn't flagged as a breaking change during code review. The PR description didn't mention the Kafka event schema change.

4. **Why wasn't the breaking change caught in CI?**  
   Because we don't have automated schema compatibility checks for Kafka events. Our contract tests only cover REST APIs.

5. **Why don't we have schema compatibility checks for events?**  
   Because when we adopted Kafka, we decided to use plain JSON serialization for simplicity. We haven't implemented a schema registry yet despite it being on the roadmap for Q3.

### Contributing Factors

- The Order API and order-processor are owned by the same team, creating a false sense of safety
- The Kafka event schema was only documented in code comments, not in a formal contract
- The deploy happened right before a meeting, reducing the deployer's monitoring window

---

## What Went Well

1. **Fast detection** — Kafka consumer lag alert fired within 2 minutes
2. **Fast response** — On-call acknowledged and triaged within 10 minutes
3. **Clear runbook** — The "Kafka Consumer Lag" runbook guided investigation
4. **No data loss** — Kafka retention ensured all events were preserved
5. **Clean rollback** — ArgoCD rollback was smooth and took < 5 minutes

---

## What Could Be Improved

1. **No schema validation for Kafka events** — REST contracts are tested, but event contracts are not
2. **PR template doesn't prompt for event schema changes** — reviewers didn't think to check
3. **No canary for event consumers** — the crash affected 100% of consumer pods immediately
4. **Deploy monitoring window too short** — deployer didn't observe consumer health after deploy

---

## Action Items

| # | Action | Owner | Due | Status |
|---|--------|-------|-----|--------|
| 1 | Implement Confluent Schema Registry for all Kafka topics | Alex Kim | 2025-11-01 | In Progress |
| 2 | Add "Event Schema Changes" checkbox to PR template | Jordan Taylor | 2025-10-10 | Done |
| 3 | Create CI check for backward-compatible schema evolution | Sarah Chen | 2025-11-15 | Not Started |
| 4 | Implement consumer canary deployment strategy | Marcus Rivera | 2025-12-01 | Not Started |
| 5 | Add 15-minute deploy monitoring requirement to deployment checklist | Jordan Taylor | 2025-10-07 | Done |
| 6 | Document all Kafka event schemas in schema registry format | Orders Team | 2025-10-31 | In Progress |

---

## Lessons Learned

> "Making a field required in a shared event is a breaking change, even when you own both the producer and consumer. Always assume consumers deploy on their own schedule."

This incident reinforced the importance of backward-compatible schema evolution. The Kafka ecosystem provides tools for this (Schema Registry with BACKWARD compatibility mode), and we've deprioritized adopting them for too long.

**Rule going forward:** All Kafka event schema changes must be backward-compatible. New required fields must have defaults. Field removal requires a 2-sprint deprecation window.

