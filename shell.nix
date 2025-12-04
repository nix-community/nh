{
  pkgs ? import <nixpkgs> { },
}:
with pkgs;
mkShell {
  strictDeps = true;

  nativeBuildInputs = [
    cargo
    rustc

    (rustfmt.override { asNightly = true; })
    rust-analyzer-unwrapped
    clippy
    taplo

    lldb
    yaml-language-server
    cargo-nextest

    nix-output-monitor
  ];

  buildInputs = lib.optionals stdenv.isDarwin [
    libiconv
  ];

  env = {
    NH_NOM = "1";
    NH_LOG = "nh=trace";
    RUST_SRC_PATH = "${rustPlatform.rustLibSrc}";
  };
}
