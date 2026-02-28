# ADR-0019: AGPL-3.0 License

**Status:** Accepted
**Date:** 2026-02-27

## Context

Context Harness was initially released under the MIT license (the most
permissive widely-used open-source license). As the project matures and
plans for library publishing (PRD-0006), a Tauri desktop app (PRD-0008),
and distribution (PRD-0010) take shape, the permissive license creates a
concern: any entity can take the code, build a proprietary product or
managed service on top of it, and sell it without contributing back.

The project maintainer wants to prevent commercial exploitation while
keeping the project open source, welcoming contributions, and maintaining
compatibility with crates.io and the broader Rust ecosystem.

## Decision

Change the license from MIT to **GNU Affero General Public License v3.0
(AGPL-3.0-or-later)**.

AGPL-3.0 is a strong copyleft license: anyone who modifies and distributes
Context Harness (including offering it as a network service) must release
their source code under the same license. This effectively prevents
proprietary forks and managed-service exploitation while keeping the
project fully open source (OSI-approved).

The copyright holder (Parallax Labs) retains the option to offer a
separate commercial license for entities that cannot comply with AGPL
copyleft requirements. This dual-licensing model is well established
(MySQL, Qt, MongoDB pre-SSPL, Grafana).

## Alternatives Considered

**Keep MIT.** Maximum adoption and ecosystem compatibility, but no
protection against proprietary forks or commercial exploitation. Anyone
can take the code, wrap it in a SaaS, and sell it without contributing
anything back. Rejected because the maintainer explicitly wants to
prevent this.

**Elastic License 2.0 (ELv2).** Prohibits offering the software as a
managed service. Simple and readable. However, it is not OSI-approved
and is not considered "open source" by the OSI definition. Some
organizations have blanket policies against non-OSI licenses, which
would limit adoption. crates.io accepts it but the Rust ecosystem
strongly favors OSI-approved licenses.

**Business Source License 1.1 (BSL).** Source-available with a time-bomb
conversion to open source after a change date. Prevents production use
without a license. Not OSI-approved. The time-bomb mechanic adds
complexity and may confuse contributors about their rights.

**PolyForm Noncommercial 1.0.** Directly prohibits commercial use.
Very clear, but overly restrictive: it would prevent companies from using
Context Harness internally for their own work (not selling it, just using
it). This is too broad -- we want to prevent selling, not prevent using.

**PolyForm Shield 1.0.** Prevents competing with the licensor. Narrower
than Noncommercial, but still not OSI-approved and less well-understood
than AGPL.

**GPL-3.0 (without Affero clause).** Strong copyleft for distributed
software, but the "ASP loophole" means someone can run a modified version
as a network service without releasing source. AGPL closes this gap,
which is important for a tool that runs as an HTTP server (MCP).

## Consequences

- **Copyleft protection.** Anyone who modifies and distributes Context
  Harness (or offers it as a service) must release their modifications
  under AGPL-3.0. This prevents proprietary forks and managed-service
  exploitation.

- **OSI-approved.** AGPL-3.0 is recognized as open source by the OSI.
  crates.io, GitHub, and the broader ecosystem accept it without issues.

- **Contribution model.** All contributions are made under the same
  AGPL-3.0 license. Contributors retain their copyright but grant the
  same license to all recipients. CONTRIBUTING.md documents this.

- **Dual-licensing option.** The copyright holder can offer a commercial
  license for organizations that need to embed Context Harness in
  proprietary products without complying with AGPL copyleft. This is a
  standard business model.

- **Some corporate adoption friction.** Some companies have policies
  against AGPL dependencies. This is an accepted tradeoff: the project
  prioritizes protecting the codebase over maximizing corporate adoption.
  Companies that need AGPL exemption can negotiate a commercial license.

- **Dependency compatibility.** All current dependencies (tokio, sqlx,
  axum, clap, serde, mlua, fastembed, tract, etc.) are MIT or Apache-2.0
  licensed, which are compatible with AGPL-3.0 (permissive licenses can
  be included in AGPL projects).

- **Registry extensions.** Lua extensions (connectors, tools, agents) in
  the community registry are separate works that communicate with Context
  Harness via a scripting API. They are not required to be AGPL-licensed.
  The registry repository can use its own license (e.g., Apache-2.0 or
  MIT) for the extension scripts themselves.

## Related

- **PRDs:** [PRD-0006](../prd/0006-workspace-refactor-and-library-publishing.md)
  (library publishing to crates.io under AGPL),
  [PRD-0010](../prd/0010-distribution-and-packaging.md) (distribution)
- **Files changed:** `LICENSE`, `Cargo.toml`, `README.md`,
  `CONTRIBUTING.md`, `site/templates/index.html`, `site/static/llms.txt`,
  `site/content/blog/local-first-context-cursor-claude.md`
