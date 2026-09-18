# Releasing

vsg-rs is distributed on PyPI as `vsg-rs`. The wheels contain the `vsg-rs` executable (installed
into the environment's scripts directory, so it is on `PATH`) and a small `vsg_rs` Python module
(`python -m vsg_rs ...`, `vsg_rs.find_vsg_rs_bin()`). The wheels do not depend on the Python
version (`py3-none-<platform>`); they are tested on Python 3.10 and 3.14 (the oldest and newest supported).

```sh
pip install vsg-rs      # or: uv tool install vsg-rs / pipx install vsg-rs
vsg-rs --version
```

## Platforms

| Platform | Wheel tag | Tested in CI |
|---|---|---|
| Linux x86_64 (glibc ≥ 2.28) | `manylinux_2_28_x86_64` | Python 3.10 and 3.14 |
| Linux aarch64 (glibc ≥ 2.28) | `manylinux_2_28_aarch64` | build only |
| Linux x86_64 (musl) | `musllinux_1_2_x86_64` | build only |
| Linux aarch64 (musl) | `musllinux_1_2_aarch64` | build only |
| Windows x64 | `win_amd64` | Python 3.10 and 3.14 |
| Windows arm64 | `win_arm64` | build only (cross-compiled) |
| macOS arm64 (11.0+) | `macosx_11_0_arm64` | Python 3.10 and 3.14 |
| macOS x86_64 (10.12+) | `macosx_10_12_x86_64` | build only (cross-compiled) |
| other | sdist (needs a Rust toolchain ≥ 1.95 and network access for the git dependency) | built from sdist on Linux and Windows |

## Standalone binaries

Each GitHub release also has archives with just the `vsg-rs` executable (plus README and
licenses), and a `SHA256SUMS` file:

| Archive | Platform |
|---|---|
| `vsg-rs-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` | Linux x86_64, static |
| `vsg-rs-vX.Y.Z-aarch64-unknown-linux-musl.tar.gz` | Linux aarch64, static |
| `vsg-rs-vX.Y.Z-x86_64-pc-windows-msvc.zip` | Windows x64 |
| `vsg-rs-vX.Y.Z-aarch64-pc-windows-msvc.zip` | Windows arm64 |
| `vsg-rs-vX.Y.Z-aarch64-apple-darwin.tar.gz` | macOS arm64 |
| `vsg-rs-vX.Y.Z-x86_64-apple-darwin.tar.gz` | macOS x86_64 |

## Workflow

`.github/workflows/release.yml`:

1. checks that the tag `vX.Y.Z` equals the version in `Cargo.toml`;
2. builds the wheels and the sdist with maturin, and the standalone binaries with cargo
   (Linux targets with `cargo zigbuild`); native binaries are run once;
3. installs each Linux x86_64, Windows x64 and macOS arm64 wheel into Python 3.10 and 3.14,
   and builds the sdist on Linux and Windows. Each installation runs `python/tests/smoke.py`
   (console script, `python -m vsg_rs`, stdin formatting, error handling, linting, fixing with
   CRLF line endings);
4. on tags only: publishes to PyPI with trusted publishing, and creates a GitHub release with
   the wheels, the sdist, the binary archives and `SHA256SUMS` attached, with build provenance
   attestations (GitHub for all files, PEP 740 on PyPI). Attestations are skipped if the repository is private,
   because GitHub does not offer them for private repositories on its Free plan.

Manual runs (`gh workflow run release.yml`) and pull requests that touch the packaging do steps
1 to 3 only.

## crates.io

vsg-rs is not on crates.io yet: crates.io does not accept git dependencies, and vsg-rs needs
`vhdl_syntax` fixes that are newer than its 0.2.0 release (see `vhdl-frontend.md`). When a
`vhdl_syntax` release contains them, switch `Cargo.toml` to that version, create an API token
on crates.io and publish with `cargo publish` (afterwards, crates.io trusted publishing can
replace the token).

## One-time setup

1. On PyPI, add a *trusted publisher* for the project `vsg-rs`: owner `ru551n`, repository
   `vsg-rs`, workflow `release.yml`, environment `pypi` (a "pending publisher" can be created
   before the first upload).
2. In the GitHub repository settings, create the environment `pypi` (optionally with required
   reviewers).

No API tokens are stored in the repository.

## Making a release

```sh
# 1. bump `version` in Cargo.toml, run `cargo check` to update Cargo.lock
# 2. commit and push to main; wait for CI
git tag -a vX.Y.Z -m "vsg-rs X.Y.Z"
git push origin vX.Y.Z
gh run watch   # follow the Release workflow
```

## Local build

```sh
uvx maturin build --release --out dist        # wheel for this machine
uvx maturin sdist --out dist
uv venv -p 3.12 .venv && uv pip install -p .venv/bin/python dist/*.whl
PATH=.venv/bin:$PATH .venv/bin/python python/tests/smoke.py
```
