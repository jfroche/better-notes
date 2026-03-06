{ pkgs, ... }:

pkgs.buildGoModule rec {
  pname = "cclog";
  version = "0.7.4";

  src = pkgs.fetchFromGitHub {
    owner = "annenpolka";
    repo = "cclog";
    rev = "v${version}";
    hash = "sha256-X2aDCGjSZyjOyqEwtSEKw8EiVvtF0HpNtQ8S1XtUrWk=";
  };

  vendorHash = "sha256-RQrLFMSa38quxGeJvcz8fsfjrAXVh3LJFWhoUB3saR0=";

  subPackages = [ "cmd/cclog" ];

  meta = {
    description = "Convert Claude Code session logs to readable markdown";
    homepage = "https://github.com/annenpolka/cclog";
    platforms = pkgs.lib.platforms.linux ++ pkgs.lib.platforms.darwin;
  };
}
