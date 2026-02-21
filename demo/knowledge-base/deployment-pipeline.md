# Deployment Pipeline

Our CI/CD pipeline ensures safe, automated deployments from commit to production.

---

## Pipeline Stages

```
Commit → Build → Test → Security Scan → Deploy Staging → Canary → Production
```

### 1. Build (GitHub Actions)

Triggered on every push to `main` and on PR creation.

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build
        run: cargo build --release
      - name: Lint
        run: cargo clippy -- -D warnings
      - name: Format check
        run: cargo fmt --check
      - name: Upload artifact
        uses: actions/upload-artifact@v4
```

Build artifacts are Docker images pushed to our private ECR registry with the Git SHA as the tag.

### 2. Test Suite

Three test tiers run in parallel:

| Tier | Duration | What |
|------|----------|------|
| Unit | ~2 min | Pure function tests, mocked dependencies |
| Integration | ~8 min | Database, message queue, external service tests |
| E2E | ~15 min | Full user flow through the UI |

**Flaky test policy:** If a test fails intermittently more than 3 times in 30 days, it's quarantined and the owning team has 1 sprint to fix or delete it.

### 3. Security Scanning

- **SAST:** Semgrep rules for common vulnerabilities
- **Dependency audit:** `cargo audit` + Snyk for transitive dependencies
- **Container scan:** Trivy scans Docker images for known CVEs
- **Secret detection:** TruffleHog prevents accidental credential commits

Critical findings block deployment. High findings must be addressed within 1 sprint.

### 4. Staging Deployment

ArgoCD watches the staging branch and auto-syncs:

```yaml
apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: order-service-staging
spec:
  destination:
    namespace: staging
    server: https://kubernetes.default.svc
  source:
    repoURL: https://github.com/acme/deployments
    path: services/order-service/staging
```

Staging mirrors production topology at 1/10th scale. Synthetic traffic generators simulate real user patterns.

### 5. Canary Deployment

Production deployments use a canary strategy:

1. Deploy new version to 5% of pods
2. Monitor error rate, latency p99, and business metrics for 15 minutes
3. If metrics are healthy, ramp to 25% → 50% → 100% over 1 hour
4. Automatic rollback if error rate increases >0.5% or p99 latency increases >20%

```yaml
apiVersion: flagger.app/v1beta1
kind: Canary
spec:
  analysis:
    interval: 1m
    threshold: 5
    maxWeight: 50
    stepWeight: 10
  metrics:
    - name: request-success-rate
      thresholdRange:
        min: 99.5
    - name: request-duration
      thresholdRange:
        max: 500
```

---

## Rollback Procedures

### Automatic Rollback

Canary deployments auto-rollback when metrics breach thresholds. No human intervention needed.

### Manual Rollback

For post-deploy issues discovered after canary graduation:

```bash
# Rollback to previous version
kubectl rollout undo deployment/order-service -n production

# Or deploy a specific version
argocd app set order-service --revision <git-sha>
argocd app sync order-service
```

### Database Rollback

Database migrations are forward-only. To "rollback" a migration:

1. Create a new migration that reverts the schema change
2. Ensure the application code handles both old and new schema
3. Deploy the reverting migration
4. Deploy the application rollback

**Rule:** Never use `DROP COLUMN` or `ALTER COLUMN` in a migration without a 2-phase deploy strategy.

---

## Environment Promotion

| Environment | Purpose | Data | Scale |
|-------------|---------|------|-------|
| Dev | Local development | Synthetic | Single node |
| Staging | Integration testing | Anonymized production | 1/10th prod |
| Production | Live traffic | Real | Full scale |

Feature flags (LaunchDarkly) control feature availability independently of deployments. This decouples deploy from release.

---

## Deployment Schedule

- **Staging:** Continuous (every merge to main)
- **Production:** Business hours only (9 AM - 4 PM ET, Mon-Thu)
- **Freeze periods:** 2 weeks before major product launches, last week of each quarter
- **Emergency deploys:** Anytime with SEV-1 incident commander approval

