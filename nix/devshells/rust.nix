{
  flake,
  system,
  pkgs,
  ...
}:
let
  craneLib = flake.lib.mkCraneLib { inherit pkgs; };
in
craneLib.devShell {
  inputsFrom = [
    flake.devShells.${system}.default
  ]
  ++ (pkgs.lib.lists.optionals
    (pkgs.lib.meta.availableOn pkgs.stdenv.hostPlatform flake.packages.${system}.better-notes)
    (
      [
        flake.packages.${system}.better-notes
      ]
      ++ (builtins.attrValues flake.packages.${system}.better-notes.passthru.tests)
    )
  );

  packages = [
    pkgs.cargo-nextest
    pkgs.cargo-watch
  ];

  RUST_LOG = "better_notes=debug";
}
