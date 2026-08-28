let
  hm = builtins.getFlake "github:nix-community/home-manager";
  pkgs = import <nixpkgs> { };
in
import "${hm}/modules" {
  inherit pkgs;
  configuration = {
    home.stateVersion = "24.05";
    home.packages = [ pkgs.hello ];
    home.username = "anon";
    home.homeDirectory = "/anonfiles";
  };
}
