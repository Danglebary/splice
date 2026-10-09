{
  description = "The splice development shell, every tool the gate runs, and the splice binary as a package, pinned by the lock.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
      # The toolchain is the one `rust-toolchain.toml` pins, so CI's rustup and this
      # shell resolve the same compiler from one file.
      toolchainFor = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      shellFor =
        system:
        let
          pkgs = pkgsFor system;
        in
        pkgs.mkShell {
          # Set inside this shell alone, so a recipe can tell it already stands in it
          # and need not enter it again.
          SPLICE_SHELL = "1";
          packages = [
            (toolchainFor pkgs)
            pkgs.just
          ];
        };
      # The binary, built by the pinned toolchain from the manifest, its lock, and the
      # crate's sources alone. The tests are the gate's, so the build runs none.
      packageFor =
        system:
        let
          pkgs = pkgsFor system;
          toolchain = toolchainFor pkgs;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
          fileset = pkgs.lib.fileset;
          manifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        in
        rustPlatform.buildRustPackage {
          pname = "splice";
          version = manifest.package.version;
          src = fileset.toSource {
            root = ./.;
            fileset = fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./src
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          doCheck = false;
          meta.mainProgram = "splice";
        };
    in
    {
      devShells = nixpkgs.lib.genAttrs systems (system: {
        default = shellFor system;
      });
      packages = nixpkgs.lib.genAttrs systems (system: {
        default = packageFor system;
      });
    };
}
