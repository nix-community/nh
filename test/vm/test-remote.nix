{
  lib,
  pkgs,
  # Inherited from flake.nix
  nixpkgs,
  nh,
  ...
}:
let
  modulesPath = "${nixpkgs}/nixos/modules";
  inherit (import ../ssh-keys.nix pkgs) snakeOilPrivateKey snakeOilPublicKey;

  sshConfig = builtins.toFile "ssh.conf" ''
    UserKnownHostsFile=/dev/null
    StrictHostKeyChecking=no
  '';

  # Base configuration for target
  targetBaseConfig = {
    documentation.enable = false;
    services.openssh.enable = true;
    system.switch.enable = true;
  };

  # Configuration file generator
  mkConfigFile =
    hostname:
    pkgs.writeText "configuration-${hostname}.nix" ''
      import <nixpkgs/nixos> {
        configuration = {
          imports = [
            ./hardware-configuration.nix
            "${modulesPath}/profiles/installation-device.nix"
          ];

          boot.loader.grub = {
            enable = true;
            device = "/dev/vda";
            forceInstall = true;
          };

          documentation.enable = false;
          services.openssh.enable = true;
          system.switch.enable = true;

          networking.hostName = "${hostname}";

          environment.systemPackages = with pkgs; [ hello ];
        };
      }
    '';
in
pkgs.testers.nixosTest {
  name = "nh-remote-test";
  meta.maintainers = with lib.maintainers; [ NotAShelf ];

  nodes = {
    deployer =
      {
        lib,
        pkgs,
        ...
      }:
      {
        imports = [ "${modulesPath}/profiles/installation-device.nix" ];

        nix.settings = {
          substituters = lib.mkForce [ ];
          hashed-mirrors = null;
          connect-timeout = 1;
          experimental-features = [
            "nix-command"
            "flakes"
          ];
        };

        virtualisation = {
          cores = 2;
          memorySize = 3072;
        };

        system = {
          includeBuildDependencies = true;
          switch.enable = true;
          build.privateKey = snakeOilPrivateKey;
          build.publicKey = snakeOilPublicKey;
        };

        services.openssh.enable = true;
        environment.systemPackages = [ nh ];
        users.users.root.openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
      };

    target =
      {
        nodes,
        lib,
        ...
      }:
      {
        virtualisation = {
          cores = 2;
          memorySize = 2048;
          vlans = [ 1 ];
        };

        nix.settings = {
          substituters = lib.mkForce [ ];
          experimental-features = [
            "nix-command"
            "flakes"
          ];
        };

        system.switch.enable = true;

        users.users.root.openssh.authorizedKeys.keys = [ nodes.deployer.system.build.publicKey ];

        services.openssh.enable = true;
        environment.systemPackages = [ nh ];
        networking.hostName = "target";
      };

    buildHost =
      {
        nodes,
        lib,
        ...
      }:
      {
        virtualisation = {
          cores = 2;
          memorySize = 2048;
          vlans = [ 1 ];
        };

        nix.settings = {
          substituters = lib.mkForce [ ];
          experimental-features = [
            "nix-command"
            "flakes"
          ];
        };

        system.switch.enable = true;
        users.users.root.openssh.authorizedKeys.keys = [ nodes.deployer.system.build.publicKey ];

        services.openssh.enable = true;
        environment.systemPackages = [ nh ];
        networking.hostName = "buildHost";
      };
  };

  testScript = ''
    start_all()

    # Wait for all nodes to be ready
    deployer.wait_for_unit("multi-user.target")
    target.wait_for_unit("sshd.service")
    buildHost.wait_for_unit("sshd.service")

    # Setup SSH keys on deployer
    deployer.succeed("mkdir -p /root/.ssh")
    deployer.succeed("install -m 600 ${snakeOilPrivateKey} /root/.ssh/id_ecdsa")
    deployer.succeed("install ${sshConfig} /root/.ssh/config")

    # Get IP addresses from VLAN interface (eth1)
    # Yeesh.
    target_ip = target.succeed("ip -4 addr show eth1 | grep -oP '(?<=inet\\s)\\d+(\\.\\d+){3}'").strip()
    build_host_ip = buildHost.succeed("ip -4 addr show eth1 | grep -oP '(?<=inet\\s)\\d+(\\.\\d+){3}'").strip()

    print(f"Target IP: {target_ip}")
    print(f"Build host IP: {build_host_ip}")

    # Setup known_hosts
    deployer.succeed(f"ssh-keyscan {target_ip} >> /root/.ssh/known_hosts")
    deployer.succeed(f"ssh-keyscan {build_host_ip} >> /root/.ssh/known_hosts")

    # Test SSH connectivity
    deployer.succeed(f"ssh root@{target_ip} 'echo SSH to target works'")
    deployer.succeed(f"ssh root@{build_host_ip} 'echo SSH to buildHost works'")

    # Generate hardware configuration on target and verify it exists
    target.succeed("nixos-generate-config --dir /root")
    target.succeed("ls -la /root/hardware-configuration.nix")  # Debug: verify file exists
    deployer.succeed(f"scp root@{target_ip}:/root/hardware-configuration.nix /root/hardware-configuration.nix")

    # Copy test configurations to deployer
    deployer.copy_from_host("${mkConfigFile "config-1-deployed"}", "/root/configuration-1.nix")
    deployer.copy_from_host("${mkConfigFile "config-2-deployed"}", "/root/configuration-2.nix")
    deployer.copy_from_host("${mkConfigFile "config-3-deployed"}", "/root/configuration-3.nix")

    with subtest("Local build and switch on target"):
        # Copy config to target for local build
        deployer.succeed(f"scp /root/configuration-1.nix root@{target_ip}:/root/configuration.nix")
        deployer.succeed(f"scp /root/hardware-configuration.nix root@{target_ip}:/root/hardware-configuration.nix")

        # Build locally on target using non-flake syntax
        target.succeed("nh os switch --bypass-root-check -f '<nixpkgs/nixos>'")

        # Verify hostname changed
        target_hostname = target.succeed("cat /etc/hostname").strip()
        assert target_hostname == "config-1-deployed", f"Expected 'config-1-deployed', got '{target_hostname}'"

        # Verify hello package is available
        target.succeed("hello --version")

    # Build on deployer, activate on target
    with subtest("Remote build on deployer, deploy to target with --target-host"):
        deployer.succeed(f"nh os switch --bypass-root-check -f '<nixpkgs/nixos>' --target-host root@{target_ip}")

        # Verify hostname changed
        target_hostname = target.succeed("cat /etc/hostname").strip()
        assert target_hostname == "config-2-deployed", f"Expected 'config-2-deployed', got '{target_hostname}'"

    # Build on buildHost, activate on target (both different from deployer)
    with subtest("Remote build on buildHost with --build-host, deploy to target with --target-host"):
        deployer.succeed(
            f"nh os switch --bypass-root-check -f '<nixpkgs/nixos>' --build-host root@{build_host_ip} --target-host root@{target_ip}"
        )

        # Verify hostname changed
        target_hostname = target.succeed("cat /etc/hostname").strip()
        assert target_hostname == "config-3-deployed", f"Expected 'config-3-deployed', got '{target_hostname}'"

    with subtest("Remote build and deploy to same host (build-host == target-host)"):
        # Reset target to config-1 first
        deployer.succeed(f"nh os switch --bypass-root-check -f '<nixpkgs/nixos>' --target-host root@{target_ip}")

        # Build and deploy on target itself via deployer
        deployer.succeed(
            f"nh os switch --bypass-root-check -f '<nixpkgs/nixos>' --build-host root@{target_ip} --target-host root@{target_ip}"
        )

        # Verify hostname changed
        target_hostname = target.succeed("cat /etc/hostname").strip()
        assert target_hostname == "config-2-deployed", f"Expected 'config-2-deployed', got '{target_hostname}'"

    with subtest("Build-only operation with --build-host (no activation)"):
        # Just build, don't activate
        deployer.succeed(f"nh os build --bypass-root-check -f '<nixpkgs/nixos>' --build-host root@{build_host_ip}")

        # Verify build succeeded by checking result link exists
        deployer.succeed("test -L result")

        # Verify target hostname didn't change (still config-2)
        target_hostname = target.succeed("cat /etc/hostname").strip()
        assert target_hostname == "config-2-deployed", f"Hostname should not have changed, got '{target_hostname}'"

    with subtest("Fail when running as root without --bypass-root-check"):
        # Attempt to run as root without the bypass flag - should fail
        target.fail("nh os switch -f '<nixpkgs/nixos>'")
  '';
}
