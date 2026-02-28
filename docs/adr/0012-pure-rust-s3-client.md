# ADR-0012: Pure Rust S3 Client

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness includes an S3 connector for ingesting documents from
Amazon S3 and S3-compatible storage (MinIO, Backblaze B2, DigitalOcean
Spaces). The S3 integration must:

- Sign requests with AWS Signature Version 4 (SigV4)
- Support `ListObjectsV2` with continuation-token pagination
- Support custom endpoints for S3-compatible services
- Build cleanly on all six targets including Nix-based builds
- Not introduce large dependency trees or native library requirements

## Decision

Implement S3 access with a **hand-rolled SigV4 signing layer** using the
`hmac` and `sha2` crates, and `reqwest` (with `rustls-tls`) for HTTP.

The S3 connector (`src/connector_s3.rs`) implements:

- AWS SigV4 request signing (canonical request, string-to-sign, signing key
  derivation, authorization header)
- `ListObjectsV2` API call with `continuation-token` pagination
- `GetObject` for fetching document content
- Custom endpoint support via config (`endpoint` key)
- Region, bucket, prefix, and credential configuration via `ctx.toml` with
  `${VAR}` expansion for secrets

Credentials are read from config values (which support env var expansion):

```toml
[connectors.s3.reports]
bucket = "my-reports"
prefix = "weekly/"
region = "us-east-1"
access_key_id = "${AWS_ACCESS_KEY_ID}"
secret_access_key = "${AWS_SECRET_ACCESS_KEY}"
```

## Alternatives Considered

**aws-sdk-s3 (official AWS SDK for Rust).** The most feature-complete option,
supporting every S3 API, credential chains, retry policies, and endpoint
discovery. However, it pulls in a large dependency tree (~50+ crates),
including `aws-smithy-*` code generators. It transitively depends on
OpenSSL in some configurations, which conflicts with the rustls-only policy
(see [ADR-0017](0017-rustls-over-openssl.md)) and causes Nix build failures.
The SDK's complexity is disproportionate to the needs — Context Harness only
uses `ListObjectsV2` and `GetObject`.

**rusoto.** A community AWS SDK for Rust that was popular before the official
SDK. It is now deprecated and unmaintained. Not a viable long-term choice.

**AWS CLI subprocess.** Shell out to `aws s3 ls` and `aws s3 cp`. Simple but
requires the AWS CLI to be installed, introduces subprocess management, and
makes error handling fragile. Violates the single-binary, zero-dependency
principle.

**s3-client crate (community).** Various community S3 crates exist but most
are thin wrappers around reqwest with varying quality, maintenance status,
and S3-compatibility coverage. A hand-rolled implementation gives full
control and avoids depending on an unmaintained crate.

## Consequences

- Zero additional native dependencies. The S3 connector is pure Rust using
  crates (`hmac`, `sha2`, `reqwest`) that are already in the dependency tree
  for other features.
- The SigV4 implementation is scoped to the two API calls Context Harness
  needs. It does not attempt to be a general-purpose AWS SDK, which keeps
  the code small and auditable (~300 lines).
- Custom endpoint support enables S3-compatible services (MinIO, Backblaze
  B2) without code changes — only a config value.
- The implementation does not support IAM instance profiles, STS temporary
  credentials, or credential chain resolution. Users must provide explicit
  access key and secret key. This is acceptable for the target use case
  (personal/team tools) but would need extending for enterprise environments
  with role-based access.
- Nix builds work without any special overlays or patches for OpenSSL,
  since the entire HTTP stack uses rustls.
