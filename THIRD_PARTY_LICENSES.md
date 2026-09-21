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
many editors ship.

The other themes there, all dark, use the colour palettes of the open-source colour schemes below.
Each colour value was taken from the scheme's own source at the commit shown, and
`editors/vscode/scripts/palettes.json` records which scheme every theme comes from.
`npm test` in `editors/vscode` regenerates the themes from that file and fails if a theme differs
from it.

| Themes | Colours from | License | Copyright |
|---|---|---|---|
| Deus VHDL | [ajmwagar/vim-deus](https://github.com/ajmwagar/vim-deus) at `1be965e7bc1c` | MIT | Avery Wagar |
| Moonfly VHDL | [bluz71/vim-moonfly-colors](https://github.com/bluz71/vim-moonfly-colors) at `2c883835f5b7` | MIT | vim-moonfly-colors authors |
| Catppuccin VHDL Mocha, Catppuccin VHDL Macchiato, Catppuccin VHDL Frappe | [catppuccin/palette](https://github.com/catppuccin/palette) at `07d02aa110ef` | MIT | Catppuccin |
| Afterglow VHDL | [danilo-augusto/vim-afterglow](https://github.com/danilo-augusto/vim-afterglow) at `fe3a0c4d2acf` | MIT | Danilo Augusto dos S. R. de Faria |
| Dracula VHDL | [dracula/vim](https://github.com/dracula/vim) at `e7817b4baccf` | MIT | Dracula Theme |
| Nightfox VHDL, Duskfox VHDL, Nordfox VHDL, Terafox VHDL, Carbonfox VHDL | [EdenEast/nightfox.nvim](https://github.com/EdenEast/nightfox.nvim) at `4dacd3f0185a` | MIT | James Simpson |
| Tokyo Night VHDL Storm, Tokyo Night VHDL Night, Tokyo Night VHDL Moon | [folke/tokyonight.nvim](https://github.com/folke/tokyonight.nvim) at `cdc07ac78467` | Apache-2.0 | folke |
| Tender VHDL | [jacoborus/tender.vim](https://github.com/jacoborus/tender.vim) at `b66dc330aff9` | MIT | Jacobo Tabernero |
| Blue Moon VHDL | [kyazdani42/blue-moon](https://github.com/kyazdani42/blue-moon) at `ed4ed60abeb8` | None stated | kyazdani42 |
| VS Code Dark+ VHDL | [Mofiqul/vscode.nvim](https://github.com/Mofiqul/vscode.nvim) at `6439ed89d0e1` | MIT | Mofiqul Islam |
| One Dark VHDL | [navarasu/onedark.nvim](https://github.com/navarasu/onedark.nvim) at `df4792accde9` | MIT | Navarasu |
| PaperColor VHDL | [NLKNguyen/papercolor-theme](https://github.com/NLKNguyen/papercolor-theme) at `0cfe64ffb24c` | MIT | Nikyle Nguyen |
| Oxocarbon VHDL | [nyoom-engineering/oxocarbon.nvim](https://github.com/nyoom-engineering/oxocarbon.nvim) at `cd6523a0836d` | MIT | Riccardo Mazzarini |
| One Dark Pro VHDL, One Dark Pro VHDL Vivid, One Dark Pro VHDL Dark | [olimorris/onedarkpro.nvim](https://github.com/olimorris/onedarkpro.nvim) at `24c806cb85c1` | MIT | Oli Morris |
| Koda Moss VHDL | [oskarnurm/koda.nvim](https://github.com/oskarnurm/koda.nvim) at `25a5c98ca2f6` | MIT | Karl Oskar Joosep Nurm |
| Kanagawa VHDL Wave, Kanagawa VHDL Dragon | [rebelot/kanagawa.nvim](https://github.com/rebelot/kanagawa.nvim) at `bb85e4bfc8d8` | MIT | Tommaso Laurenzi |
| Bamboo VHDL Vulgaris, Bamboo VHDL Multiplex | [ribru17/bamboo.nvim](https://github.com/ribru17/bamboo.nvim) at `1309bc88bffc` | MIT | Navarasu, Riley Bruins |
| Everforest VHDL Hard, Everforest VHDL Medium, Everforest VHDL Soft | [sainnhe/everforest](https://github.com/sainnhe/everforest) at `85a86eb62409` | MIT | sainnhe |
| Gruvbox Material VHDL Hard, Gruvbox Material VHDL Medium, Gruvbox Material VHDL Soft | [sainnhe/gruvbox-material](https://github.com/sainnhe/gruvbox-material) at `11d779b26a9a` | MIT | sainnhe |
| Torchlight VHDL, Torchlight VHDL Dusk | [skylarmb/torchlight.nvim](https://github.com/skylarmb/torchlight.nvim) at `3347ef0164ef` | MIT | Skylar Brown |
| Vague VHDL | [vague-theme/vague.nvim](https://github.com/vague-theme/vague.nvim) at `f9060fdaf7b2` | MIT | Alberto Hernandez |
| Unokai VHDL | [vim/colorschemes](https://github.com/vim/colorschemes) at `8b294bc5e0b3` | None stated | k-37 |
| Dogrun VHDL | [wadackel/vim-dogrun](https://github.com/wadackel/vim-dogrun) at `8205b05312da` | MIT | wadackel |
| Luna VHDL | [WTFox/luna.nvim](https://github.com/WTFox/luna.nvim) at `727c19334528` | MIT | A. Fox |

Only colour values are used: no theme source, grammar or code is copied from these or any other
project, and the themes are otherwise vsg-rs's own, under MIT OR Apache-2.0. Unokai and Blue Moon
come from repositories that state no licence; they are credited to their authors, and only their
colour values are used. Where a scheme computes a colour (a blend or a shade of its base colours),
the theme carries the computed value.

The MIT-licensed schemes are used under this notice, which applies to the colour values taken from
each of them, with the copyright holder shown in the table:

> Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
> associated documentation files (the "Software"), to deal in the Software without restriction,
> including without limitation the rights to use, copy, modify, merge, publish, distribute,
> sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or
> substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
> NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

Tokyo Night is licensed under the [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0),
and its colour values are used under those terms.
