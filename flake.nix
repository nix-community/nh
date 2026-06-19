{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?ref=nixos-25.11";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      inherit (nixpkgs) lib;

      pkgsFor = system: nixpkgs.legacyPackages.${system} or (import nixpkgs { inherit system; });

      supportedSystems = builtins.filter (
        system: (builtins.tryEval (pkgsFor system).stdenv.hostPlatform).success
      ) (lib.systems.doubles.linux ++ lib.systems.doubles.darwin);

      forAllSystems = function: lib.genAttrs supportedSystems (system: function (pkgsFor system));

      ciSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      rev = self.shortRev or self.dirtyShortRev or "dirty";
    in
    {
      overlays.default = final: _: {
        nh = final.callPackage ./package.nix {
          inherit crane rev;
          pkgs = final;
        };
      };

      packages = forAllSystems (pkgs: {
        nh = pkgs.callPackage ./package.nix {
          inherit crane rev;
          pkgs = pkgs;
        };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.nh;
      });

      checks = lib.genAttrs ciSystems (system: self.packages.${system});

      devShells = forAllSystems (pkgs: {
        default = import ./shell.nix { inherit pkgs; };
      });

      formatter = forAllSystems (
        pkgs:
        # Provides the default formatter for 'nix fmt', which will format the
        # entire tree with Nixfmt. Treefmt is *wildly* overkill for this project
        # so a simple bash script will suffice.
        pkgs.writeShellApplication {
          name = "nix3-fmt-wrapper";

          runtimeInputs = [
            pkgs.nixfmt
            pkgs.fd
          ];

          text = ''
            # Find Nix files in the tree and format them with Alejandra
            fd "$@" -t f -e nix -x nixfmt -q '{}'
          '';
        }
      );
    };
}
