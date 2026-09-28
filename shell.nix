{
  pkgs ? import <nixpkgs> { },
}:
with pkgs;
let
  # Wild supports x86_64 and AArch64 Linux.
  hasWild =
    stdenv.hostPlatform.isLinux && (stdenv.hostPlatform.isx86_64 || stdenv.hostPlatform.isAarch64);
in
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
    just
    nix-output-monitor

    # Markdown formatting
    deno
  ]
  ++ lib.optionals hasWild [
    clang
    wild
  ];

  buildInputs = lib.optionals stdenv.hostPlatform.isDarwin [
    libiconv
  ];

  env = {
    NH_NOM = "1";
    NH_LOG = "nh=trace";
    RUST_SRC_PATH = "${rustPlatform.rustLibSrc}";
  }
  // lib.optionalAttrs hasWild {
    RUSTFLAGS = "-Clinker=${clang}/bin/clang -Clink-arg=--ld-path=wild";
  };
}
