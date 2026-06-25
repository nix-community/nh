{
  inputs.nixpkgs.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.xz";

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      inherit (nixpkgs) lib;

      pkgsFor = system: nixpkgs.legacyPackages.${system} or (import nixpkgs { inherit system; });

      supportedSystems = lib.systems.doubles.linux ++ lib.systems.doubles.darwin;

      forAllSystems = function: lib.genAttrs supportedSystems (system: function (pkgsFor system));

      ciSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      rev = self.shortRev or self.dirtyShortRev or "dirty";
    in
    {
      overlays.default = final: _: { nh = final.callPackage ./package.nix { inherit rev; }; };

      packages = forAllSystems (pkgs: {
        nh = pkgs.callPackage ./package.nix { inherit rev; };
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
            pkgs.nixfmt-rfc-style
            pkgs.taplo
            pkgs.deno
            pkgs.fd
          ];

          text = ''
            # Format Nix with Nixfmt
            fd "$@" -t f -e nix -x nixfmt -q '{}'

            # Format TOML with Taplo
            fd "$@" -t f -e toml -x taplo fmt '{}'

            # Format Markdown with Deno
            fd "$@" -t f -e md -x deno fmt -q '{}'
          '';
        }
      );
    };
}
