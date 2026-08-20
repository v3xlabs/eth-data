{
  lib,
  writeShellApplication,
  bubblewrap,
  coreutils,
  cacert,
  foundry,
  eth-data-mcp,
}:
writeShellApplication {
  name = "eth-data-mcp-sandboxed";
  runtimeInputs = [bubblewrap coreutils];
  text = ''
    workspace="$(mktemp -d -p "''${XDG_RUNTIME_DIR:-/tmp}" eth-data-mcp.XXXXXX)"
    trap 'rm -rf "$workspace"' EXIT

    # --share-net is load bearing. fork_create binds anvil to 127.0.0.1 and hands
    # the caller that URL to drive with cast or forge. An unshared network
    # namespace would keep the sandbox's loopback private, so every fork would
    # start correctly and then be unreachable from the caller's shell.
    bwrap \
      --die-with-parent \
      --unshare-all \
      --share-net \
      --new-session \
      --clearenv \
      --dir /nix \
      --ro-bind /nix/store /nix/store \
      --dir /etc \
      --ro-bind /etc/resolv.conf /etc/resolv.conf \
      --proc /proc \
      --dev /dev \
      --tmpfs /tmp \
      --bind "$workspace" /workspace \
      --chdir /workspace \
      --setenv HOME /workspace/home \
      --setenv PATH "${lib.makeBinPath [foundry coreutils]}" \
      --setenv SSL_CERT_FILE "${cacert}/etc/ssl/certs/ca-bundle.crt" \
      -- ${lib.getExe eth-data-mcp} "$@"
  '';

  meta = {
    description = "eth-data-mcp in a bubblewrap sandbox with a temporary workspace";
    platforms = lib.platforms.linux;
  };
}
