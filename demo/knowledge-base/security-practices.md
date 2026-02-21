# Security Practices

Security is everyone's responsibility. These practices are mandatory for all engineers.

---

## Authentication & Authorization

### Identity Provider

All human access goes through Okta SSO with mandatory MFA:

- **Primary factor:** Okta Verify push notification
- **Backup factors:** TOTP codes, hardware security keys (YubiKey)
- **Session duration:** 12 hours for standard access, 1 hour for admin access
- **Conditional access:** VPN required for production access from non-corporate networks

### Service Identity

Services authenticate using short-lived certificates managed by the Istio service mesh:

1. Each pod gets a unique SPIFFE identity: `spiffe://acme.com/ns/production/sa/order-service`
2. Certificates auto-rotate every 24 hours
3. mTLS enforced for all inter-service communication
4. Authorization policies define which services can communicate

```yaml
apiVersion: security.istio.io/v1
kind: AuthorizationPolicy
metadata:
  name: order-service-policy
spec:
  selector:
    matchLabels:
      app: order-service
  rules:
    - from:
        - source:
            principals: ["cluster.local/ns/production/sa/api-gateway"]
      to:
        - operation:
            methods: ["GET", "POST"]
            paths: ["/v1/orders*"]
```

---

## Secrets Management

### HashiCorp Vault

All secrets are stored in Vault and injected at runtime:

```bash
# Application reads secrets from Vault agent sidecar
vault kv get secret/order-service/database
```

### Secret Rotation

| Secret Type | Rotation Period | Automated |
|-------------|-----------------|-----------|
| Database passwords | 30 days | Yes |
| API keys | 90 days | Yes |
| TLS certificates | 24 hours | Yes (cert-manager) |
| Encryption keys | 365 days | Yes (AWS KMS) |

### Never Do This

- ❌ Commit secrets to Git (even in private repos)
- ❌ Store secrets in environment variables in CI
- ❌ Share secrets via Slack or email
- ❌ Use long-lived API keys for service accounts
- ❌ Hardcode credentials in application code

---

## Data Protection

### Encryption

| Layer | Method | Key Management |
|-------|--------|----------------|
| At rest | AES-256 | AWS KMS |
| In transit | TLS 1.3 | cert-manager |
| Application-level | AES-256-GCM | Vault Transit |

### PII Handling

Personal Identifiable Information requires special handling:

1. **Classification:** All data fields must be classified (PII, Sensitive, Internal, Public)
2. **Minimization:** Only collect PII that is strictly necessary
3. **Tokenization:** PII is tokenized before storage; original values in a separate token vault
4. **Retention:** PII deleted after retention period (default: 2 years, configurable per regulation)
5. **Access logging:** All PII access is logged and auditable

### GDPR Compliance

- Right to erasure: automated deletion pipeline triggered via admin API
- Data portability: export endpoint returns all user data in JSON format
- Consent tracking: granular consent stored per data processing purpose
- Cross-border transfers: EU data stays in eu-west-1 region

---

## Vulnerability Management

### Dependency Scanning

```bash
# Run in CI on every PR
cargo audit                    # Rust advisory database
snyk test --all-projects      # Transitive dependency scan
trivy image acme/order:latest  # Container vulnerability scan
```

### Severity Response Times

| Severity | Response | Fix Deadline |
|----------|----------|-------------|
| Critical (CVSS 9.0+) | Immediate page | 24 hours |
| High (CVSS 7.0-8.9) | Same-day triage | 1 week |
| Medium (CVSS 4.0-6.9) | Sprint planning | 1 sprint |
| Low (CVSS 0.1-3.9) | Backlog | Next quarter |

### Penetration Testing

- Annual third-party pentest of all external-facing services
- Quarterly internal red team exercises
- Bug bounty program (HackerOne) with payouts up to $10,000

---

## Incident Security Response

If you suspect a security incident:

1. **Do not try to fix it yourself** — contain and escalate
2. Notify `#security-incidents` Slack channel immediately
3. Page the security on-call: `pd trigger security`
4. Preserve evidence — do not delete logs or modify systems
5. Document everything with timestamps

### Security Incident Classification

| Type | Example | Escalation |
|------|---------|------------|
| Data breach | PII exposed externally | CEO, Legal, CISO |
| Unauthorized access | Suspicious login patterns | Security team |
| Malware | Compromised dependency | Security + Platform |
| Social engineering | Phishing targeting engineers | Security + HR |

