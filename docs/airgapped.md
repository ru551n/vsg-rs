# Running in an airgap

vsg-rs is a formatter and a linter. It reads the files you give it and writes the files you ask
it to fix. It does not fetch anything, phone anywhere, or execute the VHDL it reads.

That is a claim, and a claim is worth what it can be checked against. Everything on this page is
either a job that fails the build in CI, or a command you can run yourself in a few minutes
against the artifact you received. None of it asks you to trust how the code was written.

## What it needs at run time

Nothing but the input. The `ieee` and `std` libraries are compiled into the binary, so the lint
layer resolves `std_logic_vector` with no simulator installed and no library path configured. A
project's own configuration and its `vhdl_ls.toml` are ordinary files, found by walking up from
the file being checked.

There is no licence check, no update check, no telemetry and no crash reporter.

It writes one thing you did not name. The lint layer resolves names through a library mapping
that is file based, so on its first run it unpacks the embedded `ieee` and `std` sources into
`$XDG_CACHE_HOME/vsg-rs/vhdl_libraries-<version>/` (or `$HOME/.cache/...`, or the temporary
directory if neither is set): 39 files, about 2.3 MB, written once per version and skipped when
they are already there. The style layer writes nothing at all. Deleting that directory costs one
extra second on the next lint run and nothing else.

## It makes no network calls

Three separate checks, all in the `airgap` job of `.github/workflows/ci.yml`, run on every pull
request that touches the Rust code:

**No dependency is a network client.** The dependency tree is listed and matched against the
crates that speak HTTP or TLS (`reqwest`, `hyper`, `curl`, `rustls`, `openssl` and the rest). A
match fails the build. `tokio` is in the tree for the language server's stdio transport, built
without its `net` feature, so it has no sockets to offer.

**The binary imports no symbol that can reach a host.** `nm -D` must not show `socket`,
`connect`, `bind`, `listen`, `accept`, `sendto`, `recvfrom`, `getaddrinfo` or `gethostbyname`.
`socketpair` is permitted and present: it connects two processes on the same machine and cannot
name a remote host.

**It runs with no network at all.** The binary is exercised inside an empty network namespace:
the command line, the rule set, `--fix`, `--explain`, `--list_rules` and the MCP server. The job
first proves the isolation is real, by checking that no interface but `lo` exists and that a TCP
connection to a public address fails, so a namespace that leaked a route fails the job rather
than passing it quietly.

**It does not even try.** The same run is traced with `strace -e trace=%network`, and any network
syscall other than `socketpair` fails the build. That is the stronger statement: not "no network
was available", but "it never asked for one".

### Checking it yourself

On the machine you are importing to, against the binary you received:

```sh
nm -D ./vsg-rs | grep -E ' U (socket|connect|getaddrinfo)@'   # expect no output
strace -f -qq -e trace=%network -o net.log ./vsg-rs --check style,lint file.vhd
grep -v socketpair net.log                                    # expect no output
sudo unshare -n ./vsg-rs --check style,lint file.vhd          # expect it to work
```

The static Linux builds have no dynamic symbols to list, so for those the `strace` and `unshare`
checks are the ones that apply.

## Getting it in

Two forms, both self-contained:

* the **wheel**, installed with no index at all:

    ```sh
    pip install --no-index --only-binary :all: --find-links . vsg-rs
    ```

    This is how the release workflow installs and tests every wheel it builds, so the offline
    path is the tested path rather than an afterthought.

* the **standalone binary** from the release page. The Linux builds are static (musl): no glibc
  version to match and no shared libraries to carry.

**Not the sdist.** Building from source needs a Rust toolchain and network access, because the
parser is pinned to a git revision that `cargo` fetches. Carry a wheel or a binary.

## Verifying what you received

Each release has a `SHA256SUMS` file, and every file carries a build provenance attestation:

```sh
sha256sum -c SHA256SUMS
gh attestation verify vsg-rs-v0.11.0-x86_64-unknown-linux-musl.tar.gz --repo ru551n/vsg-rs
```

The attestation ties the artifact to the workflow run and the commit that produced it, so "this
is the file that repository built from that tag" is a check rather than a hope. Wheels on PyPI
carry the same thing as PEP 740 attestations. Everything the build pulls in is pinned: actions by
commit hash, crates by `Cargo.lock` with every CI command run `--locked`, the parser by git
revision, and each CI tool by version.

Licences are MIT or Apache-2.0, with every dependency's licence checked in CI by `cargo-deny` and
recorded in `THIRD_PARTY_LICENSES.md`.

## What it executes

Nothing, with one exception, stated plainly.

The VHDL is parsed and analysed, never run. vsg-rs is not a simulator and has no evaluator.

The exception is `--local_rules` (or `local_rules:` in a configuration), which exists for VSG's
Python rule plugins. Only VSG can run those, so vsg-rs runs the `vsg` on your `PATH` as a
subprocess. Without that option no subprocess is started. With it, you are running your own VSG
install and your own plugins, and that is the one thing here that is exactly as trustworthy as
whatever you put there.

## On the code being generated

Most of this repository was written by a language model. That is a reason to check it rather than
to trust it, and none of the arguments above depend on it having been written well:

* the network claims are syscall traces and symbol tables, which do not care who wrote the code;
* **output is verified before it is returned.** Formatted source is re-parsed and must contain
  exactly the same tokens and comments, in the same order, attached to the same tokens. A
  mismatch is reported as an internal error and your file is left untouched, so a formatter bug
  costs you a run rather than a file;
* a file with a syntax error is never modified;
* files are written atomically, through a temporary file in the same directory, and a file whose
  output equals its input is not rewritten at all;
* the lint layer is held to reporting nothing on real projects unless a person has confirmed each
  finding is a genuine defect, and the counts are a gate in CI rather than a note in a review;
* agreement with VSG's own behaviour is measured weekly over a corpus and published in
  [compatibility](compatibility.md).

The strongest argument for an airgap needs none of that, though: the tool reads a file, writes a
file, and cannot reach anything else. That is checkable in an afternoon, and the checks are
listed above.
