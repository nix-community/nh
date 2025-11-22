{
  lib,
  rustPlatform,
  installShellFiles,
  makeBinaryWrapper,
  use-nom ? true,
  nix-output-monitor ? null,
  rev ? "dirty",
}:
assert use-nom -> nix-output-monitor != null;
let
  runtimeDeps = lib.optionals use-nom [ nix-output-monitor ];
  cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = "nh";
  version = "${cargoToml.workspace.package.version}-${rev}";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.intersection (lib.fileset.fromSource (lib.sources.cleanSource ./.)) (
      lib.fileset.unions [
        ./.cargo
        ./src
        ./xtask
        ./Cargo.toml
        ./Cargo.lock
      ]
    );
  };

  strictDeps = true;

  nativeBuildInputs = [
    installShellFiles
    makeBinaryWrapper
  ];

  postInstall = ''
    mkdir completions man

    for shell in bash zsh fish nu; do
      NH_NO_CHECKS=1 $out/bin/nh completions $shell > completions/nh.$shell
    done

    installShellCompletion completions/*

    cargo xtask man --out-dir gen
    installManPage gen/nh.1
  '';

  postFixup = ''
    wrapProgram $out/bin/nh \
      --prefix PATH : ${lib.makeBinPath runtimeDeps}
  '';

  cargoLock.lockFile = ./Cargo.lock;

  env.NH_REV = rev;

  meta = {
    description = "Yet another nix cli helper";
    homepage = "https://github.com/nix-community/nh";
    license = lib.licenses.eupl12;
    mainProgram = "nh";
    maintainers = with lib.maintainers; [
      drupol
      NotAShelf
      viperML
    ];
  };
}
