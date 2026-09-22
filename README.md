<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/ru551n/speja/main/docs/assets/speja-logo-dark.png">
    <img src="https://raw.githubusercontent.com/ru551n/speja/main/docs/assets/speja-logo.png" alt="" width="128">
  </picture>
</p>

# vsg-rs is now speja

This project continues at **[github.com/ru551n/speja](https://github.com/ru551n/speja)**. This
repository is archived and read-only. Its history is preserved here, and also carried over to the
new one, so the commits below exist in both places.

*speja* is Swedish for keeping watch ahead, which is what the tool does: it reports what it can
prove about your VHDL before simulation is asked to find it. The old name tied the project to
VSG, and it had grown past being a port of it.

## Moving over

| | was | now |
|---|---|---|
| Install | `pip install vsg-rs` | `pip install speja` |
| Command | `vsg-rs` | `speja` |
| Configuration | `vsg-rs.yaml`, `.vsg-rs.yaml` | `speja.yaml`, `.speja.yaml` |
| Waivers | `vsg-rs-waivers.yaml` | `speja-waivers.yaml` |
| VS Code settings | `vsg-rs.*` | `speja.*` |
| GitHub Action | `ru551n/vsg-rs@v0.11.0` | `ru551n/speja@v0.13.0` |
| Documentation | vsg-rs.readthedocs.io | [speja.readthedocs.io](https://speja.readthedocs.io/) |
| Extension | not published | [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=ru551n.speja) and [Open VSX](https://open-vsx.org/extension/ru551n/speja) |

Rename your configuration and waiver files, install the new package, and remove the old one. The
rules, their ids, the configuration format, the report formats and the VSG compatibility are all
unchanged, so a pipeline needs only the name changed.

`pip install vsg-rs` keeps working and stays at 0.11.1 forever. The two packages install
executables of different names, so they can sit side by side while you move.

## Licence

MIT OR Apache-2.0, unchanged.
