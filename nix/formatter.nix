{
  pkgs,
  inputs,
  perSystem,
  ...
}:
let
  settingsNix = {
    package = perSystem.nixpkgs.treefmt;

    projectRootFile = ".git/config";

    programs = {
      nixfmt.enable = true;
      deadnix = {
        enable = true;
        no-underscore = true;
      };
      statix.enable = true;

      rustfmt = {
        enable = true;
        edition = "2021";
      };

      taplo.enable = true;

      just.enable = true;
    };

    settings = {
      global.excludes = [
        "LICENSE"
        "*.lock"
        "*.envrc"
        "*.gitignore"
      ];

      formatter = {
        deadnix = {
          priority = 1;
        };

        nixfmt = {
          priority = 2;
        };

        statix = {
          priority = 3;
        };
      };
    };
  };

  treefmtEval = inputs.treefmt-nix.lib.evalModule pkgs settingsNix;

in
treefmtEval.config.build.wrapper.overrideAttrs (_: {
  passthru = {
    inherit (treefmtEval.config) package settings;
    inherit (treefmtEval) config;
    inherit settingsNix;
  };
})
