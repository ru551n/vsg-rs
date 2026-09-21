# Third-party licenses

vsg-rs itself is licensed under MIT OR Apache-2.0. It links the following crates (runtime
dependencies, generated with `cargo metadata`). Their license texts are distributed with each
crate. No third-party source is copied into this repository.

Notable: `vhdl_syntax` (the VHDL parser from the VHDL-LS/rust_hdl project) is MPL-2.0. It is used
unmodified as a Cargo dependency; MPL-2.0 is file-level copyleft, so modifications to its files
would have to be published under MPL-2.0, while vsg-rs code is unaffected.

| Crate | Version | License |
|---|---|---|
| aho-corasick | 1.1.5 | Unlicense OR MIT |
| anstream | 1.0.0 | MIT OR Apache-2.0 |
| anstyle | 1.0.14 | MIT OR Apache-2.0 |
| anstyle-parse | 1.0.0 | MIT OR Apache-2.0 |
| anstyle-query | 1.1.5 | MIT OR Apache-2.0 |
| anstyle-wincon | 3.0.11 | MIT OR Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| bstr | 1.13.1 | MIT OR Apache-2.0 |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 |
| clap | 4.6.7 | MIT OR Apache-2.0 |
| clap_builder | 4.6.7 | MIT OR Apache-2.0 |
| clap_derive | 4.6.7 | MIT OR Apache-2.0 |
| clap_lex | 1.1.1 | MIT OR Apache-2.0 |
| colorchoice | 1.0.5 | MIT OR Apache-2.0 |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 |
| either | 1.18.0 | MIT OR Apache-2.0 |
| equivalent | 1.0.2 | Apache-2.0 OR MIT |
| errno | 0.3.14 | MIT OR Apache-2.0 |
| fastrand | 2.5.0 | Apache-2.0 OR MIT |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| heck | 0.5.0 | MIT OR Apache-2.0 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| is_terminal_polyfill | 1.70.2 | MIT OR Apache-2.0 |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| libc | 0.2.189 | MIT OR Apache-2.0 |
| libyaml-rs | 0.3.0 | MIT |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| memchr | 2.8.3 | Unlicense OR MIT |
| nonzero_ext | 0.3.0 | Apache-2.0 |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| once_cell_polyfill | 1.70.2 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| rayon | 1.12.0 | MIT OR Apache-2.0 |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 |
| regex | 1.13.1 | MIT OR Apache-2.0 |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT |
| rustix | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.151 | MIT OR Apache-2.0 |
| similar | 3.2.0 | Apache-2.0 |
| strsim | 0.11.1 | MIT |
| syn | 3.0.5 | MIT OR Apache-2.0 |
| tempfile | 3.27.0 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| utf8parse | 0.2.2 | Apache-2.0 OR MIT |
| vhdl_syntax | 0.2.0 | MPL-2.0 |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| yaml_serde | 0.10.7 | MIT OR Apache-2.0 |
| zmij | 1.0.23 | MIT |

## The VS Code extension's themes

The two *Gruvbox VHDL* colour themes in `editors/vscode/themes` use the colour palette of
[Gruvbox](https://github.com/morhetz/gruvbox), the open-source theme by morhetz (MIT/X11), which
many editors ship. Only its colour values are used: no theme source, grammar or code is copied
from it or from any other project, and the themes are otherwise vsg-rs's own, under
MIT OR Apache-2.0.
