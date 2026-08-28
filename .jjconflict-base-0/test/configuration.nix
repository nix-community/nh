{ pkgs, ... }:
{
  fileSystems."/" = {
    device = "none";
    fsType = "tmpfs";
    options = [ "size=1G" ];
  };

  boot.loader.grub.devices = [ "nodev" ];

  environment.systemPackages = [ pkgs.hello ];
}
