+++
title = "Telemetry-free forever"
description = "Context Harness is removing product telemetry and committing to a simpler rule: your tools should not phone home unless you explicitly ask them to."
date = 2026-06-08

[taxonomies]
tags = ["privacy", "local-first", "open-source"]
+++

Context Harness is a local-first context tool. It indexes your docs, notes, source code, runbooks, and project memory so your AI tools can work with the knowledge you already have.

That makes privacy more than a feature checkbox. It is part of the product's shape.

So we are removing telemetry from Context Harness, and we are making the policy simple:

**Context Harness will stay telemetry-free forever.**

## What changed

Earlier builds included anonymous product analytics. The implementation avoided personal data, used a random local identifier, showed a notice, and had opt-out switches.

That still was not the right default for this project.

The right default is no analytics client, no background event delivery, no local analytics identity, and no telemetry state file. When you run `ctx init`, `ctx sync`, `ctx search`, or `ctx serve mcp`, Context Harness should do the work you asked it to do and nothing else.

## Why

Context Harness often runs against private corpora:

- source code
- planning docs
- design notes
- customer runbooks
- personal writing
- local AI memory

Even when telemetry does not collect that content, a networked analytics path changes the trust boundary. It asks users to believe the implementation, the configuration, the dependency, the hosted service, and every future change to that path.

We would rather remove the path.

The local-first promise is stronger when the product does not need a carve-out for product analytics. Your context stays local unless you choose a connector, embedding provider, or deployment mode that explicitly talks to another service.

## How we will learn instead

Telemetry is convenient, but convenience is not the only way to build a product.

We will learn from:

- issues and discussions
- pull requests
- benchmark reports
- user-written configs and examples
- docs feedback
- explicit bug reports
- release download signals from GitHub

That is slower and less tidy than a dashboard. It is also more aligned with what Context Harness is supposed to be: inspectable infrastructure for people who care where their context goes.

## The rule

Context Harness may still make network requests when you configure networked behavior: cloning a Git repository, reading S3, using OpenAI embeddings, downloading a local embedding model, installing a registry, or serving an endpoint you start.

Those are user-directed actions.

Telemetry is different. Telemetry is the product deciding to report back about itself. We are not going to do that.

No anonymous usage analytics. No product event stream. No tracking ID. No phone-home behavior hidden behind a friendly paragraph.

Local-first should feel calm. This is one way we keep it that way.
