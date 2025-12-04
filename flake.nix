{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      forAllSystems =
        function:
        nixpkgs.lib.genAttrs [
          "x86_64-linux"
          "aarch64-linux"
          "x86_64-darwin"
          "aarch64-darwin"
        ] (system: function nixpkgs.legacyPackages.${system});

      rev = self.shortRev or self.dirtyShortRev or "dirty";
    in
    {
      overlays.default = final: _: { nh = final.callPackage ./package.nix { inherit rev; }; };

      packages = forAllSystems (pkgs: {
        nh = pkgs.callPackage ./package.nix { inherit rev; };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.nh;
      });

      checks = self.packages // self.devShells;

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
            pkgs.nixfmt-rfc-style
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
