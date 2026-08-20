{
  lib,
  rustPlatform,
  makeWrapper,
  foundry,
  withAnvil ? true,
}:
rustPlatform.buildRustPackage {
  pname =
    if withAnvil
    then "eth-data-mcp"
    else "eth-data-mcp-unwrapped";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ../.;
    filter = path: type: let
      name = baseNameOf path;
    in
      !(builtins.elem name ["target" "result" ".tmp" ".direnv" ".git"]);
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = lib.optionals withAnvil [makeWrapper];

  # The fork tools spawn `anvil`. Without it on PATH `nix run` starts a server
  # whose fork_create always fails, which reads as a broken install.
  postInstall = lib.optionalString withAnvil ''
    wrapProgram "$out/bin/eth-data-mcp" \
      --prefix PATH : "${lib.makeBinPath [foundry]}"
  '';

  # The integration tests reach the network and spawn anvil.
  doCheck = false;

  meta = {
    description = "A read-only Model Context Protocol server for Ethereum data";
    homepage = "https://github.com/v3xlabs/eth-data";
    license = lib.licenses.lgpl3Only;
    mainProgram = "eth-data-mcp";
    platforms = lib.platforms.unix;
  };
}
