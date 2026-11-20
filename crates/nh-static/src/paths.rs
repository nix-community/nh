//! Well-known filesystem paths.

use crate::Documented;

/// System profile, holding the system generations of NixOS and nix-darwin.
pub const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";

/// Symlink to the currently active system.
pub const CURRENT_SYSTEM: &str = "/run/current-system";

/// Contains the name of the active NixOS specialisation, if any.
pub const SPECIALISATION: &str = "/etc/specialisation";

/// Garbage-collector roots registered automatically by Nix, such as `result`
/// symlinks.
pub const AUTO_GCROOTS: &str = "/nix/var/nix/gcroots/auto";

/// Paths documented in nh's man page, in order.
pub const DOCUMENTED: &[Documented] = &[
  Documented {
    name: SYSTEM_PROFILE,
    doc:  "System profile directory containing system generations.",
  },
  Documented {
    name: CURRENT_SYSTEM,
    doc:  "Symlink to the currently active system profile.",
  },
  Documented {
    name: SPECIALISATION,
    doc:  "NixOS specialisation detection. Contains the active specialisation \
           name.",
  },
];
