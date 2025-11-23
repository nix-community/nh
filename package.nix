{
  lib,
  stdenv,
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
  cargoToml = lib.importTOML ./Cargo.toml;
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
  nativeBuildInputs = [ makeBinaryWrapper ];

  cargoLock.lockFile = ./Cargo.lock;

  postInstall = lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
    # Run both shell completion and manpage generation tasks. Unlike the
    # fine-grained variants, the 'dist' command doesn't allow specifying the
    # path but that's fine, because we can simply install them from the implicit
    # output directories.
    cargo xtask dist

    # The dist task above should've created
    #  1. Shell completions in comp/
    #  2. The NH manpage (nh.1) in man/
    # Let's install those.
    for dir in comp man; do
      mkdir -p "$out/share/$dir"
      cp -rf "$dir" "$out/share/"
    done
  '';

  postFixup = ''
    wrapProgram $out/bin/nh \
      --prefix PATH : ${lib.makeBinPath runtimeDeps}
  '';

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
