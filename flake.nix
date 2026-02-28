{
  description = "Context Harness — local-first context engine for AI tools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: import nixpkgs { inherit system; };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          manifest = (pkgs.lib.importTOML ./crates/context-harness/Cargo.toml).package;
        in
        rec {
          # Default: no local embeddings so nix run works in sandbox (no network).
          # ort-sys (used by fastembed) needs to download ONNX Runtime at build time; sandboxed Nix builds have no network.
          default = no-local-embeddings;

          no-local-embeddings = pkgs.rustPlatform.buildRustPackage {
            pname = manifest.name;
            version = manifest.version;

            src = self;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            buildNoDefaultFeatures = true;

            # Binary is named ctx (Cargo [[bin]] name), not context-harness (package name).
            meta.mainProgram = "ctx";

            # Integration tests run git (init, clone, etc.)
            nativeCheckInputs = [ pkgs.git ];
          };

          # Full build with local embeddings (fastembed). Requires network at build time so ort-sys can download ONNX Runtime.
          # If sandboxed build fails, try: nix build --option sandbox false .#with-embeddings  OR  nix develop && cargo build
          with-embeddings = pkgs.rustPlatform.buildRustPackage {
            pname = "${manifest.name}-full";
            version = manifest.version;

            src = self;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            buildInputs = pkgs.lib.optional pkgs.stdenv.isLinux pkgs.openssl
              ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.libcxx;
            nativeBuildInputs = pkgs.lib.optional pkgs.stdenv.isLinux pkgs.pkg-config;
            meta.mainProgram = "ctx";

            # On Darwin: libcxx in buildInputs gives the linker -lc++ so deps (e.g. ort/fastembed) link.
            # Do not set CC/CXX to Zig here: some deps run `zig build` when zig is on PATH, which fails without build.zig.
            nativeCheckInputs = [ pkgs.git ];
          };
        });

      devShells = forAllSystems (system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              rustPackages.clippy
              pkg-config
              openssl
            ]
              ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.zig;

            # So openssl-sys finds OpenSSL when building with default features (Linux)
            OPENSSL_DIR = pkgs.lib.optionalString pkgs.stdenv.isLinux "${pkgs.openssl.dev}";

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          }
          // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
            # Use Zig as CC/C++ so linking uses Zig's toolchain and avoids -lc++ path issues.
            CC = ''"${pkgs.zig}/bin/zig" cc'';
            CXX = ''"${pkgs.zig}/bin/zig" c++'';
          };
        });
    };
}
