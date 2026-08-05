# chapbook 的 Nix 打包（rustPlatform 标准方式，cargoLock 锁定依赖）
# 用法：nix build .#chapbook 或作为 flake 输入引用
{ lib, rustPlatform }:

let
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;
in
rustPlatform.buildRustPackage {
  pname = "chapbook";
  inherit version;

  # cleanSource 尊重 .gitignore（排除 target/ 等），并剔除 .git
  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  # 默认 checkPhase 即 cargo test（测试无外部依赖，nix 沙箱内可跑）
  meta = with lib; {
    description = "Serve a directory as a readable little book — directory listing, static files, Markdown/Org rendering with syntax highlighting";
    homepage = "https://github.com/weiwen99/chapbook";
    license = licenses.asl20;
    mainProgram = "chapbook";
  };
}
