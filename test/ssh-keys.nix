pkgs: {
  # This key is used in integration tests
  # This is NOT a security issue
  # It uses the test key defined in RFC 9500
  # https://datatracker.ietf.org/doc/rfc9500/
  snakeOilPrivateKey = pkgs.writeText "privkey.snakeoil" ''
    -----BEGIN EC PRIVATE KEY-----
    MHcCAQEEIObLW92AqkWunJXowVR2Z5/+yVPBaFHnEedDk5WJxk/BoAoGCCqGSM49
    AwEHoUQDQgAEQiVI+I+3gv+17KN0RFLHKh5Vj71vc75eSOkyMsxFxbFsTNEMTLjV
    uKFxOelIgsiZJXKZNCX0FBmrfpCkKklCcg==
    -----END EC PRIVATE KEY-----
  '';

  snakeOilPublicKey = pkgs.lib.concatStrings [
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHA"
    "yNTYAAABBBEIlSPiPt4L/teyjdERSxyoeVY+9b3O+XkjpMjLMRcWxbEzRDEy41b"
    "ihcTnpSILImSVymTQl9BQZq36QpCpJQnI= snakeoil"
  ];
}
