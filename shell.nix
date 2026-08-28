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
    just
    # Markdown formatting
    deno
  ];

  buildInputs = lib.optionals stdenv.hostPlatform.isDarwin [
    libiconv
  ];

  env = {
    NH_LOG = "nh=trace";
    RUST_SRC_PATH = "${rustPlatform.rustLibSrc}";
  };
}
