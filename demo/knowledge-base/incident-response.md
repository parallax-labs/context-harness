# Incident Response Playbook

## Severity Levels

### SEV-1: Critical Production Outage
- **Definition:** Complete loss of service for all users, data loss risk, or security breach
- **Response time:** 15 minutes
- **Escalation:** VP Engineering + On-call SRE + affected team leads
- **Communication:** Status page updated every 15 minutes, exec Slack channel, customer success notified

### SEV-2: Major Degradation
- **Definition:** Significant feature unavailable, performance degraded >50%, data integrity concerns
- **Response time:** 30 minutes
- **Escalation:** On-call SRE + team lead
- **Communication:** Status page updated every 30 minutes, engineering Slack channel

### SEV-3: Minor Issue
- **Definition:** Non-critical feature affected, workaround available, isolated impact
- **Response time:** 4 hours (business hours)
- **Escalation:** Team on-call
- **Communication:** Team Slack channel, tracked in Jira

---

## On-Call Rotation

Teams rotate weekly, Monday to Monday. The on-call engineer must:

1. Acknowledge pages within 5 minutes
2. Have laptop and reliable internet access at all times
3. Be within 15 minutes of a workstation
4. Escalate if unable to triage within 30 minutes
5. Hand off to next on-call with a written summary

### Compensation
- $500/week flat on-call stipend
- Additional $200 for each SEV-1 incident handled
- Comp time: 4 hours off for each overnight page

---

## Incident Timeline Template

```
INCIDENT: [Short description]
SEVERITY: SEV-[1/2/3]
STARTED: [Timestamp UTC]
DETECTED: [Timestamp UTC] — [How detected: alert/customer report/monitoring]
ACKNOWLEDGED: [Timestamp UTC] — [Who]
MITIGATED: [Timestamp UTC] — [What action]
RESOLVED: [Timestamp UTC]
DURATION: [Total time]

IMPACT:
- Users affected: [number/percentage]
- Revenue impact: [estimate]
- Data loss: [yes/no, details]

ROOT CAUSE:
[Description]

TIMELINE:
[HH:MM] — Event description
[HH:MM] — Event description

ACTION ITEMS:
- [ ] [Description] — Owner: [name] — Due: [date]
```

---

## Post-Incident Review Process

Every SEV-1 and SEV-2 incident requires a blameless post-mortem within 48 hours.

### Rules
1. **Blameless culture** — Focus on systems, not individuals
2. **5 Whys** — Dig into root causes, not symptoms
3. **Action items must be tracked** — Each item gets a Jira ticket with an owner and due date
4. **Share learnings** — Post-mortem document shared in #engineering and presented at weekly all-hands

### Post-Mortem Template
- Incident summary
- Timeline of events
- Root cause analysis (5 Whys)
- What went well
- What could be improved
- Action items with owners and deadlines

---

## Common Runbooks

### Database Connection Pool Exhaustion
1. Check current connections: `SELECT count(*) FROM pg_stat_activity;`
2. Identify long-running queries: `SELECT * FROM pg_stat_activity WHERE state = 'active' ORDER BY query_start;`
3. Kill idle connections older than 10 minutes
4. Verify PgBouncer pool settings
5. Check for connection leaks in application logs

### Kafka Consumer Lag
1. Check consumer group lag: `kafka-consumer-groups --describe --group <group>`
2. Verify consumer health in Grafana dashboard
3. Check for poison messages in dead letter queue
4. Scale consumer instances if processing throughput is the bottleneck
5. Verify schema compatibility if deserialization errors are present

### Memory Pressure on Kubernetes Pods
1. Check pod memory usage: `kubectl top pods -n <namespace>`
2. Review recent deployments for memory regression
3. Check for memory leaks using heap profiler
4. Adjust resource limits if growth is expected
5. Consider horizontal pod autoscaler (HPA) configuration

