{
  pkgs,
  flake,
  system,
}:
let
  cclog = pkgs.callPackage ../../nix/packages/cclog.nix { };
in
pkgs.mkShell {
  packages = [
    flake.formatter.${system}
    pkgs.jq
    pkgs.nil
    pkgs.just

    # Forge CLI tools for PR status
    pkgs.gh
    pkgs.tea
    pkgs.glab

    # Claude Code session conversion
    cclog
  ];

  env = { };

  shellHook = "";
}
