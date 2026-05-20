# Vespertide for Zed

> Language support for Vespertide schema files — by **[DevFive](https://devfive.kr)**.

Brings first-class editing for Vespertide JSON and YAML schemas to the [Zed editor](https://zed.dev) by wiring up the `vespertide-lsp` language server.

## Features

- **Diagnostics** — surfaces schema validation errors and unresolved references inline.
- **Hover** — column-type and constraint documentation on hover.
- **Go to Definition** — jump to `ref_table` / enum definitions across model files.
- **Completion** — context-aware suggestions for column types, references, and enum values.
- **Drift Detection** — flags models that have diverged from the applied migration history. _Unique to Vespertide._

## Installation

### From the Zed extensions registry

Once published, open the command palette and run:

```
zed: extensions
```

Search for **Vespertide** and click _Install_.

### Local development (dev extension)

Clone this repository and from the Zed command palette run:

```
zed: install dev extension
```

Point the picker at `apps/zed-extension/`. The extension builds to WebAssembly and downloads the `vespertide-lsp` binary from the latest [GitHub Release](https://github.com/dev-five-git/vespertide/releases) on first use.

If a `vespertide-lsp` binary is already on your `PATH` (for example via `cargo install vespertide-cli` with the LSP feature, or a local debug build), the extension uses it directly — no download.

## Configuration

By default the extension matches files ending in `.vespertide`, `.vespertide.json`, `.vespertide.yaml`, and `.vespertide.yml`. To opt-in additional globs (for example the conventional `models/**/*.json` layout) add this to your Zed `settings.json`:

```json
{
  "file_types": {
    "Vespertide JSON": ["models/**/*.json"],
    "Vespertide YAML": ["models/**/*.yaml", "models/**/*.yml"]
  }
}
```

Per-project overrides go in `.zed/settings.json` at the repository root.

## Requirements

- Zed `0.155.0` or later (extension API `0.7`).
- One of:
  - A `vespertide-lsp` binary on `PATH`, **or**
  - Network access on first launch so the extension can pull the latest release asset from GitHub.

## License

Apache-2.0. See [LICENSE](./LICENSE).
