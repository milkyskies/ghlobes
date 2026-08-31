# Changelog

## [0.2.0](https://github.com/milkyskies/ghlobes/compare/v0.1.0...v0.2.0) (2026-08-31)


### ⚠ BREAKING CHANGES

* remove --update-claude-md flag, drop inline agent-rule generation

### Features

* add autopilot eligibility gate to ready ([2201fd1](https://github.com/milkyskies/ghlobes/commit/2201fd1d68681b06fa08a4f517757249f41c7c0c))
* **eligibility:** make the autopilot label the whole gate ([#10](https://github.com/milkyskies/ghlobes/issues/10)) ([64c3818](https://github.com/milkyskies/ghlobes/commit/64c3818c5d33904ed29f910ec598d748422f2d90))
* **init:** add --yes for unattended runs ([#12](https://github.com/milkyskies/ghlobes/issues/12)) ([0252d2c](https://github.com/milkyskies/ghlobes/commit/0252d2cb09346d661d71901741b0b82871c8b8c2))
* **init:** create and repair the full status option set ([83e3551](https://github.com/milkyskies/ghlobes/commit/83e3551063851273e74567203f0094cb2279b27b))
* remove --update-claude-md flag, drop inline agent-rule generation ([21c1dc5](https://github.com/milkyskies/ghlobes/commit/21c1dc588f08589d1b5e3f9785e52b76f436f034))
* ship the graph-aware commands (path, next, done, stuck, deps, closed, tree) ([c998a04](https://github.com/milkyskies/ghlobes/commit/c998a041f634d330699ba49fc852c5a1c60df611))


### Bug Fixes

* **done:** report dependents of an already-closed issue ([0438078](https://github.com/milkyskies/ghlobes/commit/0438078546627d4fc1d4a5cc13135932e2ca231b))
* **init:** describe Needs Decision by what it means, not who caused it ([e0f5bda](https://github.com/milkyskies/ghlobes/commit/e0f5bda05fa50229cbc2c0297d564b581e7984d7))
* **init:** order status options by workflow, not arrival ([#12](https://github.com/milkyskies/ghlobes/issues/12)) ([a8f0332](https://github.com/milkyskies/ghlobes/commit/a8f0332785786539b9072a21510d8b043c1cc692))
* make ready an allowlist on Todo and exclude epics ([7c035b8](https://github.com/milkyskies/ghlobes/commit/7c035b86735d59135743f3902258ea7074bef5ed))
