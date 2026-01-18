{
  description = "better-notes - Tools for enhancing daily notes";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?ref=nixos-unstable";
    blueprint.url = "github:numtide/blueprint";
    blueprint.inputs.nixpkgs.follows = "nixpkgs";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";

    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs:
    let
      blueprint = inputs.blueprint {
        inherit inputs;
        prefix = "nix/";

        nixpkgs.config.allowUnfree = false;
      };
    in
    blueprint
    // {
      packages = builtins.mapAttrs (
        _system: pkgs: pkgs // { default = pkgs.better-notes; }
      ) blueprint.packages;
    };
}
