# Team Topology & Ownership

Acme Engineering is organized around Team Topologies principles with stream-aligned teams, platform teams, and enabling teams.

---

## Stream-Aligned Teams

These teams own end-to-end business capabilities and deliver value directly to users.

### Orders Team

**Mission:** Own the order lifecycle from cart to delivery  
**Lead:** Jordan Taylor  
**Size:** 6 engineers  
**Stack:** Rust (backend), React (frontend), PostgreSQL, Kafka  

**Owns:**
- Order creation and management API
- Payment processing integration (Stripe)
- Order fulfillment workflow
- Order status notifications
- Returns and refunds

**Key Metrics:**
- Order success rate: > 99.5%
- Checkout latency p99: < 2 seconds
- Refund processing time: < 24 hours

---

### Search & Discovery Team

**Mission:** Help users find what they're looking for  
**Lead:** Wei Zhang  
**Size:** 5 engineers  
**Stack:** Rust (ranking engine), Elasticsearch, Python (ML models)  

**Owns:**
- Search API and ranking algorithm
- Product recommendations
- Browse and filter experience
- Search analytics and A/B testing
- Autocomplete and spell correction

**Key Metrics:**
- Search latency p99: < 500ms
- Click-through rate on first page: > 35%
- Zero-results rate: < 5%

---

### User Experience Team

**Mission:** Authentication, profiles, and personalization  
**Lead:** Rachel Adams  
**Size:** 4 engineers  
**Stack:** TypeScript (Next.js), PostgreSQL, Redis  

**Owns:**
- User registration and authentication
- Profile management
- Notification preferences
- Wishlist and saved items
- Personalization signals

---

## Platform Team

### Infrastructure & Platform

**Mission:** Make it easy and safe for stream-aligned teams to deliver  
**Lead:** Alex Kim  
**Size:** 7 engineers  
**Stack:** Kubernetes, Terraform, Nix, Go, Rust  

**Owns:**
- Kubernetes cluster management
- CI/CD pipeline (GitHub Actions → ArgoCD)
- Service mesh (Istio)
- Container registry and build systems
- Developer experience tooling
- Cost optimization

**SLAs:**
- CI build time: < 10 minutes for 95th percentile
- Deployment pipeline: < 30 minutes from merge to production
- Platform availability: > 99.99%

---

### Data Platform

**Mission:** Enable data-driven decisions across all teams  
**Lead:** Priya Patel  
**Size:** 5 engineers  
**Stack:** Snowflake, dbt, Airflow, Kafka, Flink, Python  

**Owns:**
- Data warehouse (Snowflake)
- ETL pipelines and data quality
- Feature store (Feast)
- ML training and serving infrastructure
- Data governance and access control

---

## Enabling Teams

### SRE Team

**Mission:** Reliability across all production services  
**Lead:** Marcus Rivera  
**Size:** 4 engineers  

**Provides:**
- On-call support and incident management
- SLO/SLI definitions and monitoring
- Capacity planning
- Chaos engineering exercises
- Reliability consulting for new services

**Engagement model:** SRE reviews all new service launches and provides reliability guidance. Teams with services below SLO get embedded SRE support.

---

### Security Team

**Mission:** Protect our systems and customer data  
**Lead:** Nadia Hassan  
**Size:** 3 engineers  

**Provides:**
- Security architecture reviews
- Penetration testing
- Vulnerability management
- Compliance (SOC 2, GDPR)
- Security training and awareness

---

## Service Ownership Matrix

| Service | Owning Team | On-Call | Dependencies |
|---------|-------------|---------|--------------|
| Order API | Orders | Orders | Payment, Inventory |
| Payment Service | Orders | Orders | Stripe API |
| Search API | Search | Search | Elasticsearch, ML |
| Ranking Engine | Search | Search | Feature Store |
| User Service | User Experience | UX | PostgreSQL, Redis |
| Auth Service | User Experience | UX | Okta, Vault |
| API Gateway | Platform | Platform | Istio |
| Data Pipeline | Data Platform | Data | Snowflake, Kafka |

---

## Cross-Team Communication

### Dependency Requests

When your team needs something from another team:

1. Open a Jira ticket in the provider team's backlog
2. Label it `cross-team` with priority and deadline
3. Attend their sprint planning if it's urgent
4. For API changes, propose via RFC in #engineering-rfcs

### RFC Process

Major cross-team changes require an RFC:

1. Author writes RFC using template in `docs/rfcs/`
2. Post in #engineering-rfcs for async review (1 week)
3. Schedule review meeting if needed
4. Approvals required from affected team leads
5. Decision documented and shared

---

## Team Health

We measure team health quarterly using these dimensions:

1. **Delivery speed** — How quickly can the team ship value?
2. **Quality** — Error rates, incident frequency, tech debt trend
3. **Autonomy** — Can the team deliver without cross-team blockers?
4. **Learning** — Is the team growing skills and improving processes?
5. **Satisfaction** — Do team members enjoy their work?

Results are discussed in retros and shared with engineering leadership.

