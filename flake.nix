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
          manifest = (pkgs.lib.importTOML ./Cargo.toml).package;
        in
        rec {
          # Default: full build with local embeddings (fastembed; model downloads on first use).
          default = with-embeddings;

          no-local-embeddings = pkgs.rustPlatform.buildRustPackage {
            pname = manifest.name;
            version = manifest.version;

            src = self;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            buildNoDefaultFeatures = true;

            # Integration tests run git (init, clone, etc.)
            nativeCheckInputs = [ pkgs.git ];
          };

          # Full build with local-embeddings (fastembed with download-binaries; no system ORT).
          with-embeddings = pkgs.rustPlatform.buildRustPackage {
            pname = "${manifest.name}-full";
            version = manifest.version;

            src = self;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            buildInputs = pkgs.lib.optional pkgs.stdenv.isLinux pkgs.openssl;
            nativeBuildInputs = pkgs.lib.optional pkgs.stdenv.isLinux pkgs.pkg-config
              ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.zig;
            # On Darwin (Nix): use Zig as CC/C++ so linking uses Zig's toolchain and avoids -lc++ path issues.
            preBuild = pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              export CC="${pkgs.zig}/bin/zig cc"
              export CXX="${pkgs.zig}/bin/zig c++"
            '';
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
