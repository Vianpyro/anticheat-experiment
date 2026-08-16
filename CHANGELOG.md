# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Vianpyro/moba/releases/tag/v0.2.0) - 2026-08-16

### Added

- *(input)* increase aim sensitivity to 0.15 world units per count fix(lobby): adjust cursor boundary drawing to prevent invisible walls feat(main): add --confine option to restrict pointer within window fix(net): implement idle timeout and keep-alive for client-server connection test(capture): update device counts in tests to reflect new sensitivity docs(architecture): document changes to aim sensitivity and lobby behavior

### Other

- *(playtest)* add tests for bot gameplay and corpus refusal
- replace release-plz with cd.yml for release automation

## [0.1.0](https://github.com/Vianpyro/moba/releases/tag/v0.1.0) - 2026-08-15

### Added

- *(release)* implement release workflow with versioning and changelog updates
- *(replay)* [**breaking**] keep the device stream in a sealed companion the replay commits to ([#14](https://github.com/Vianpyro/moba/pull/14))
- *(replay)* [**breaking**] sign a manifest, and give a replay one format that verifies ([#8](https://github.com/Vianpyro/moba/pull/8))
- *(client)* [**breaking**] a playable client, prediction, and the consent regime ([#6](https://github.com/Vianpyro/moba/pull/6))
- *(server)* [**breaking**] play three teams of three over datagrams under the MTU ([#5](https://github.com/Vianpyro/moba/pull/5))

### Other

- implement M0, the toolchain floor and repository hygiene ([#1](https://github.com/Vianpyro/moba/pull/1))
