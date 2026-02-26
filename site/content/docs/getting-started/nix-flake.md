+++
title = "Including Context Harness in Your Nix Flake"
description = "Add Context Harness as an input to your Nix flake and use it in NixOS, Home Manager, or ad-hoc shells."
weight = 3
+++

If you manage your system or development environment with Nix flakes, you can depend on the Context Harness flake and get a reproducible `ctx` binary without installing from a release tarball. This guide shows how to add the flake as an input and use it in a few common setups.

### Flake outputs

The [context-harness flake](https://github.com/parallax-labs/context-harness) exposes:

| Output | Description |
|--------|-------------|
| `packages.<system>.default` | Full binary with local embeddings (same as `with-embeddings`). |
| `packages.<system>.with-embeddings` | Same as `default`. |
| `packages.<system>.no-local-embeddings` | Minimal binary; use with OpenAI or Ollama for embeddings. |
| `devShells.<system>.default` | Development shell with Rust, Zig (on macOS), git, and build deps. |

Supported systems: `x86_64-linux`, `aarch64-linux`, `x86_64-darwin`, `aarch64-darwin`.

---

### Add the flake as an input

In your flake (e.g. `flake.nix` for NixOS, Home Manager, or a project):

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";   # or your preferred channel
    context-harness.url = "github:parallax-labs/context-harness";
    # Optional: pin to a specific revision or tag
    # context-harness.url = "github:parallax-labs/context-harness/v0.4.2";
  };

  outputs = { self, nixpkgs, context-harness, ... } @ inputs: {
    # ...
  };
}
```

Use `context-harness.packages.<system>.default` (or `.no-local-embeddings`) wherever you need the `ctx` package.

---

### Example: NixOS system package

Expose `ctx` as a system-wide package so it’s on `PATH` for all users:

```nix
# In your NixOS configuration (e.g. configuration.nix or a module)
{ config, inputs, ... }:
{
  nixpkgs.overlays = [
    (final: prev: {
      context-harness = inputs.context-harness.packages.${prev.system}.default;
    })
  ];

  environment.systemPackages = with config.nixpkgs; [
    context-harness
  ];
}
```

If you prefer not to use an overlay, you can pass the package set explicitly:

```nix
{ config, inputs, pkgs, ... }:
let
  ctx = inputs.context-harness.packages.${pkgs.system}.default;
in
{
  environment.systemPackages = [ ctx ];
}
```

---

### Example: Home Manager

Install `ctx` for your user with Home Manager:

```nix
# In your Home Manager config (e.g. home.nix)
{ config, inputs, pkgs, ... }:
{
  home.packages = [
    inputs.context-harness.packages.${pkgs.system}.default
  ];
}
```

To use the minimal binary without local embeddings:

```nix
inputs.context-harness.packages.${pkgs.system}.no-local-embeddings
```

---

### Example: Ad-hoc shell or script

Run `ctx` without installing it into a profile:

```bash
# One-off run
nix run github:parallax-labs/context-harness#default -- --version

# Shell with ctx on PATH
nix develop github:parallax-labs/context-harness#default
ctx --help
```

---

### Example: Minimal flake that only provides ctx

If your flake exists only to hand out Context Harness (e.g. for a team or CI), you can pass through the upstream packages:

```nix
{
  description = "Context Harness wrapper";

  inputs = {
    context-harness.url = "github:parallax-labs/context-harness";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, context-harness }: {
    packages = context-harness.packages;
  };
}
```

Then `nix build .#default` (or `nix profile install .#default`) from your flake uses the upstream package.

---

### Pinning to a release

To pin to a specific release or revision, set the flake URL with a ref:

```nix
context-harness.url = "github:parallax-labs/context-harness/v0.4.2";
# Or by revision:
# context-harness.url = "github:parallax-labs/context-harness?rev=<sha>";
```

After updating the lockfile (`nix flake update` or `nix flake lock --update-input context-harness`), your build uses that exact version.

---

### Development shell

To work on Context Harness itself or on a project that uses it as a dependency, use the flake’s dev shell:

```bash
git clone https://github.com/parallax-labs/context-harness.git
cd context-harness
nix develop
cargo build --release
```

The dev shell provides Rust, Zig (on macOS for C++ linking), `pkg-config`, and OpenSSL so that `cargo build` works without installing those globally.
