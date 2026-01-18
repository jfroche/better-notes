{
  pkgs,
  flake,
  system,
}:
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
  ];

  env = { };

  shellHook = "";
}
