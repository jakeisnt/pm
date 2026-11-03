{
  description = "pm, a fast Rust project manager CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;

      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rust = pkgs.rust-bin.stable."1.91.0".default;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          };
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = "pm";
            version = "0.1.0";

            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            # aws-lc-sys (rustls' crypto backend) builds C code with cmake.
            nativeBuildInputs = [ pkgs.cmake ];

            meta = with nixpkgs.lib; {
              description = "A fast Rust project manager CLI";
              homepage = "https://github.com/jakeisnt/pm";
              license = licenses.mit;
              mainProgram = "p";
            };
          };
        });
    in
    {
      inherit packages;

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${packages.${system}.default}/bin/p";
        };
      });

      overlays.default = final: _prev: {
        pm = packages.${final.system}.default;
      };

      nixosModules.default = { pkgs, ... }:
        {
          environment.systemPackages = [ self.packages.${pkgs.system}.default ];
        };
    };
}
