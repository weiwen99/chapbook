{
  description = "chapbook — serve a directory as a readable little book";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
    in
    {
      packages = forAllSystems (system:
        let pkgs = import nixpkgs { inherit system; };
        in rec {
          chapbook = pkgs.callPackage ./nix/package.nix { };
          default = chapbook;
        });

      devShells = forAllSystems (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          # 与 nixpkgs 同版本的 cargo/rustc，避免 rustup shim 无默认 toolchain 的坑
          default = pkgs.mkShell {
            # stdenv.cc：linux 上为 gcc wrapper、darwin 上为 clang wrapper
            packages = with pkgs; [ cargo rustc clippy rustfmt stdenv.cc ];
          };
        });
    };
}
