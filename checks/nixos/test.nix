{
  testers,
  writeText,
  ...
}:
testers.runNixOSTest {
  name = "nh-nixos-test";
  nodes.machine = {
    lib,
    pkgs,
    ...
  }: {
    imports = [
      ../vm.nix
    ];

    nix.settings = {
      substituters = lib.mkForce [];
      hashed-mirrors = null;
      connect-timeout = 1;
    };

    # Indicate parent config
    environment.systemPackages = [
      (pkgs.writeShellScriptBin "parent" "")
    ];

    programs.nh = {
      enable = true;
      flake = "/etc/nixos";
    };
  };

  testScript = {nodes, ...}: let
    newConfig =
      writeText "configuration.nix" # nix

      ''
        { lib, pkgs, ... }: {
          imports = [
            ./hardware-configuration.nix
            <nixpkgs/nixos/modules/testing/test-instrumentation.nix>
          ];

          boot.loader.grub = {
            enable = true;
            device = "/dev/vda";
            forceInstall = true;
          };

          documentation.enable = false;

          environment.systemPackages = [
            (pkgs.writeShellScriptBin "parent" "")
          ];


          specialisation.foo = {
            inheritParentConfig = true;

            configuration = {...}: {
              environment.etc."specialisation".text = "foo";
            };
          };

          specialisation.bar = {
            inheritParentConfig = true;

            configuration = {...}: {
              environment.etc."specialisation".text = "bar";
            };
          };
        }
      '';
  in
    # python
    ''
      machine.start()
      machine.succeed("udevadm settle")
      machine.wait_for_unit("multi-user.target")

      machine.succeed("nixos-generate-config --flake")
      machine.copy_from_host("${newConfig}", "/etc/nixos/configuration.nix")

      with subtest("Switch to the base system"):
        machine.succeed("nh os switch --no-nom")
        machine.succeed("parent")
        machine.fail("cat /etc/specialisation/text | grep 'foo'")
        machine.fail("cat /etc/specialisation/text | grep 'bar'")

      with subtest("Switch to the foo system"):
        machine.succeed("nh os switch --no-nom --specialisation foo")
        machine.succeed("parent")
        machine.succeed("cat /etc/specialisation/text | grep 'foo'")
        machine.fail("cat /etc/specialisation/text | grep 'bar'")

      with subtest("Switch to the bar system"):
        machine.succeed("nh os switch --no-nom --specialisation bar")
        machine.succeed("parent")
        machine.fail("cat /etc/specialisation/text | grep 'foo'")
        machine.succeed("cat /etc/specialisation/text | grep 'bar'")

      with subtest("Switch into specialization using `nh os test`"):
        machine.succeed("nh os test --specialisation foo")
        machine.succeed("parent")
        machine.succeed("foo")
        machine.fail("bar")
    '';
}
