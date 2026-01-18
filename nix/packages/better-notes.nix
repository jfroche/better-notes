{
  flake,
  pkgs,
  ...
}:
let
  craneLib = flake.lib.mkCraneLib { inherit pkgs; };
  src = pkgs.lib.cleanSourceWith {
    src = flake;
    filter = path: type: (craneLib.filterCargoSources path type);
  };

  commonArgs = {
    inherit src;
    strictDeps = true;

    buildInputs = [
      pkgs.openssl
    ]
    ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
      pkgs.libiconv
      pkgs.darwin.apple_sdk.frameworks.Security
      pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
    ];

    nativeBuildInputs = [
      pkgs.pkg-config
    ];

    meta.platforms = pkgs.lib.platforms.linux ++ pkgs.lib.platforms.darwin;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (
  commonArgs
  // {
    inherit cargoArtifacts;

    doCheck = false;

    buildInputs = commonArgs.buildInputs ++ [ pkgs.makeWrapper ];

    postInstall = ''
      wrapProgram $out/bin/better-notes \
        --suffix PATH : ${
          pkgs.lib.makeBinPath [
            pkgs.git
            pkgs.gh
            pkgs.tea
            pkgs.glab
          ]
        }
    '';

    passthru.tests = {
      clippy = craneLib.cargoClippy (
        commonArgs
        // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        }
      );

      doc = craneLib.cargoDoc (
        commonArgs
        // {
          inherit cargoArtifacts;
        }
      );

      deny = craneLib.cargoDeny {
        inherit src;
      };

      nextest = craneLib.cargoNextest (
        commonArgs
        // {
          inherit cargoArtifacts;

          RUST_BACKTRACE = 1;

          partitions = 1;
          partitionType = "count";
        }
      );
    };
  }
)
