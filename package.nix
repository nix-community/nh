{
  lib,
  stdenv,
  rustPlatform,
  makeBinaryWrapper,
  installShellFiles,
  versionCheckHook,
  use-nom ? true,
  nix-output-monitor ? null,
  rev ? "dirty",
}:
assert use-nom -> nix-output-monitor != null;
let
  runtimeDeps = lib.optionals use-nom [ nix-output-monitor ];
  cargoToml = lib.importTOML ./Cargo.toml;
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "nh";
  version = "${cargoToml.workspace.package.version}-${rev}";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.intersection (lib.fileset.fromSource (lib.sources.cleanSource ./.)) (
      lib.fileset.unions [
        ./.cargo
        ./.config
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

  cargoBuildFlags = [
    "-p"
    "nh"
    "-p"
    "xtask"
  ];
  cargoLock.lockFile = ./Cargo.lock;

  postInstall = lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
    # Run both shell completion and manpage generation tasks. Unlike the
    # fine-grained variants, the 'dist' command doesn't allow specifying the
    # path but that's fine, because we can simply install them from the implicit
    # output directories.
    $out/bin/xtask dist

    # The dist task above should've created
    #  1. Shell completions in comp/
    #  2. The NH manpage (nh.1) in man/
    # Let's install those.
    # The important thing to note here is that installShellCompletion cannot
    # actually load *all* shell completions we generate with 'xtask dist'.
    # Elvish, for example isn't supported. So we have to be very explicit
    # about what we're installing, or this will fail.
    installShellCompletion --cmd ${finalAttrs.meta.mainProgram} ./comp/*.{bash,fish,zsh,nu}
    installManPage ./man/nh.1

    # Avoid populating PATH with an 'xtask' cmd
    rm $out/bin/xtask
  '';

  postFixup = ''
    wrapProgram $out/bin/nh \
      --prefix PATH : ${lib.makeBinPath runtimeDeps}
  '';

  nativeInstallCheckInputs = [ versionCheckHook ];
  doInstallCheck = false; # FIXME: --version includes 'dirty' and the hook doesn't let us change the assertion
  versionCheckProgram = "${placeholder "out"}/bin/${finalAttrs.meta.mainProgram}";
  versionCheckProgramArg = "--version";

  # Besides the install check, we have a bunch of tests to run. Nextest is
  # the fastest way of running those since it's significantly faster than
  # `cargo test`, and has a nicer UI with CI-friendly characteristics.
  useNextest = true;
  cargoTestFlags = [ "-p nh" ];

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
})
