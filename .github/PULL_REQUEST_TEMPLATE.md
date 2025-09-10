<!--
^ Please include a clear and concise description of the aim of your Pull Request above this line ^

For plugin dependency/module additions, please make sure to link the source link
of the added plugin or dependency in this section. If your pull request aims to
fix an open issue or bug, please also link the relevant issue below this
line. You may attach an issue to your pull request with `Fixes #<issue number>`
above this comment, and it will be closed when your pull request is merged.
-->

## Sanity Checking

<!--
Please check all that apply. As before, this section is not a hard requirement
but checklists with more checked items are likely to be merged faster. You may
save some time in maintainer reviews by performing self-reviews here before
submitting your pull request.

If your pull request includes any change or unexpected behaviour not covered below,
please do make sure to include it above in your description.
-->

[changelog]: https://github.com/nix-community/nh/tree/master/CHANGELOG.md

- [ ] I have updated the [changelog] as per my changes
- [ ] I have tested, and self-reviewed my code
- Style and consistency
  - [ ] I ran **`nix fmt`** to format my Nix code
  - [ ] I ran **`cargo fmt`** to format my Rust code
  - [ ] I have added appropriate documentation to new code
  - [ ] My changes are consistent with the rest of the codebase
- Correctness
  - [ ] I ran **`cargo clippy`** and fixed any new linter warnings.
- If new changes are particularly complex:
  - [ ] My code includes comments in particularly complex areas to explain the
        logic
  - [ ] I have documented the motive for those changes in the PR body or commit
        description.
- Tested on platform(s)
  - [ ] `x86_64-linux`
  - [ ] `aarch64-linux`
  - [ ] `x86_64-darwin`
  - [ ] `aarch64-darwin`

---

Add a :+1: [reaction] to [pull requests you find important].

[reaction]: https://github.blog/2016-03-10-add-reactions-to-pull-requests-issues-and-comments/
[pull requests you find important]: https://github.com/nix-community/nh/pulls?q=is%3Aopen+sort%3Areactions-%2B1-desc
