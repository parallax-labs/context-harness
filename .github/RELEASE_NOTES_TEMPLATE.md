# Release notes template (for GitHub Releases)

When drafting a new GitHub Release, use the **Release body** to add a short "What's new" and link to the relevant blog post or docs. This improves visibility and helps people watching the repo or searching for "MCP" / "Cursor".

## Template

```markdown
## What's new

- **Brief bullet** — One line describing the main change.
- **Another change** — Optional second bullet.

For details, see the [blog post](https://parallax-labs.github.io/context-harness/blog/<slug>/) / [docs](https://parallax-labs.github.io/context-harness/docs/...).

## Install

[Link to release assets or copy the install snippet from README.]
```

## Example (v0.4.2-style)

```markdown
## What's new

- **Local embeddings on every platform** — All six release targets (including Linux musl and macOS Intel) now ship with local embeddings; no ORT install required. See [Local Embeddings on Every Platform](https://parallax-labs.github.io/context-harness/blog/local-embeddings-everywhere/).

## Install

Download the latest binary for your platform from the assets below, or see the [installation guide](https://parallax-labs.github.io/context-harness/docs/getting-started/installation/).
```

Keep the body concise; link to CHANGELOG or the blog for full detail.
