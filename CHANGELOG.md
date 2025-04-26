# NH Changelog

## Unreleased

### Added

- Implement local caching for search results to improve performance on
  subsequent searches and reduce network requests. Results are stored in
  `$HOME/.cache/nh`, or in `/tmp/.cache/nh` if `$HOME` cannot be resolved.
  - add `--use-cache` flag to enable/disable caching (default: enabled).
  - add `--cache-duration` flag to configure cache expiration time (default:
    3600 seconds).
- Introduce fallback mechanisms for situations where the primary search endpoint
  fails.
  - add `--use-fallback` flag to enable/disable fallback search (default:
    `true`).
  - add `--fallback-file` flag to specify a local file (in Elasticsearch
    response format) as a fallback data source.
  - add `--fallback-endpoints` flag to specify custom http endpoints for
    fallback search, supporting `{query}` and `{channel}` template variables.
    This is useful if you are using your own search mirror.
- Enable searching entirely from a local file using the `--fallback-file` option
  when network searches are undesirable. User is responsible for supplying this
  file.
- Add environment variable counterparts for all new command-line flags.

### Removed

- Mark the nixos 24.05 channel as deprecated. `nh` will now error if a search is
  attempted using a deprecated channel.
