# Engineering Onboarding Guide

Welcome to Acme Engineering! This guide will get you productive in your first two weeks.

---

## Week 1: Setup & Orientation

### Day 1: Environment Setup

1. **Laptop configuration**
   - Install Nix package manager: `curl -L https://nixos.org/nix/install | sh`
   - Clone the dev environment: `git clone git@github.com:acme/dev-env.git`
   - Run setup: `cd dev-env && make setup`
   - This installs: Rust toolchain, Node.js 20, Python 3.12, Docker, kubectl

2. **Access provisioning**
   - GitHub: Request access to `acme` org in #it-helpdesk
   - AWS: SSO login configured via Okta — use `aws sso login --profile acme-dev`
   - Kubernetes: Kubeconfig distributed via `dev-env` setup
   - Datadog: Request access from your team lead
   - PagerDuty: Added to your team's rotation after Week 2

3. **First PR**
   - Add yourself to `team-directory.yaml`
   - This verifies your Git, CI, and code review setup works end-to-end

### Day 2-3: Architecture Overview

- Watch the "System Architecture" recording (45 min) on Loom
- Read ADR-001 through ADR-004 in the architecture-decisions doc
- Shadow a senior engineer during their daily work
- Tour of the Grafana dashboards and key metrics

### Day 4-5: First Feature

- Pick a "good first issue" from your team's backlog
- Pair with your onboarding buddy
- Ship your first feature (however small!) by end of Week 1

---

## Week 2: Deep Dive

### Team-Specific Training

Each team has a specialized onboarding track:

**Platform Team:**
- Kubernetes cluster architecture
- CI/CD pipeline (GitHub Actions → ArgoCD)
- Infrastructure-as-code (Terraform + Nix)
- On-call shadowing

**Product Team:**
- Feature flag system (LaunchDarkly)
- A/B testing framework
- Frontend architecture (React + Next.js)
- User analytics pipeline

**Data Team:**
- Data warehouse architecture (Snowflake)
- ETL pipelines (dbt + Airflow)
- Data contracts and schema evolution
- ML model serving infrastructure

---

## Development Workflow

### Branch Strategy

We use trunk-based development:

1. Create a short-lived feature branch from `main`
2. Keep branches under 400 lines changed
3. Open PR with description template
4. Require 1 approval (2 for infrastructure changes)
5. CI must pass: tests, linting, type checking, security scan
6. Squash merge to `main`
7. Auto-deploy to staging, manual promotion to production

### Code Review Guidelines

- Review within 4 business hours
- Focus on: correctness, readability, test coverage, security
- Use "nitpick:" prefix for style suggestions
- Block only for: bugs, security issues, missing tests, breaking changes

### Testing Requirements

- Unit test coverage: minimum 80% for new code
- Integration tests for all API endpoints
- E2E tests for critical user flows
- Performance benchmarks for hot paths

---

## Communication

### Meetings

- **Daily standup:** 9:15 AM, 15 minutes max
- **Sprint planning:** Monday 10 AM (bi-weekly)
- **Retro:** Friday 3 PM (bi-weekly)
- **Tech talks:** Thursday 2 PM (weekly, rotating presenter)
- **All-hands:** Tuesday 11 AM (monthly)

### Slack Channels

- `#engineering` — General engineering discussion
- `#incidents` — Active incident coordination
- `#deploys` — Deployment notifications
- `#code-review` — PR review requests
- `#til` — Today I Learned (share interesting discoveries)
- `#random` — Water cooler chat

---

## Key Contacts

| Role | Person | Slack |
|------|--------|-------|
| VP Engineering | Dana Morrison | @dana |
| Platform Lead | Alex Kim | @alexk |
| Product Lead | Jordan Taylor | @jtaylor |
| Data Lead | Priya Patel | @priya |
| SRE Lead | Marcus Rivera | @marcus |
| HR / People Ops | Sam Williams | @samw |

