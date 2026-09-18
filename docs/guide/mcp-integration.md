# MCP Server Integration

OxiDex has an MCP (Model Context Protocol) server, **oxidex-mcp**, that lets AI
assistants such as Claude read, write, search, analyze and copy file metadata
through natural language. It is a separate project with its own repository and
release cycle: [github.com/swack-tools/oxidex-mcp](https://github.com/swack-tools/oxidex-mcp).
It was split out of this repository on 2025-11-19; nothing under `oxidex/`
builds it.

## What the server provides

The server speaks JSON-RPC 2.0 over stdin/stdout and exposes five tools (from
the oxidex-mcp README):

| Tool | What it does |
| --- | --- |
| `extract_metadata` | Extract metadata from files; supports glob patterns |
| `write_metadata` | Write or update tags, with a dry-run mode |
| `search_metadata` | Find files by metadata criteria (`=`, `>`, `<`, `~` contains) |
| `analyze_metadata` | Statistical summaries of metadata across files |
| `copy_metadata` | Copy metadata between files or to several destinations |

Paths are validated (no directory traversal), there is no network access, and
write operations can be previewed with dry-run before they are applied.
Explore the 16,684 metadata tags defined in the OxiDex tag database through these tools;
the [tag coverage report](/reference/tag-coverage-analysis) states how many of
them are measured as extracted.

## Installation

Build from the oxidex-mcp repository:

```bash
git clone https://github.com/swack-tools/oxidex-mcp.git
cd oxidex-mcp
just install            # builds and installs to ~/.local/bin
# or: cargo build --release && cp target/release/oxidex-mcp ~/.local/bin/
```

See that repository's README for the current prerequisites and any pre-built
binaries.

## Configuration

Every MCP client takes the same shape of configuration: the path to the
`oxidex-mcp` binary and an empty argument list.

### Claude Desktop

Edit the client config file:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`
- Linux: `~/.config/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "oxidex": {
      "command": "/home/you/.local/bin/oxidex-mcp",
      "args": [],
      "env": { "RUST_LOG": "info" }
    }
  }
}
```

Restart Claude Desktop after editing.

### Claude Code

```bash
claude mcp add oxidex -- /home/you/.local/bin/oxidex-mcp
```

### Cline and other MCP clients

Any client that launches stdio MCP servers works; add an entry of the same
shape to its MCP configuration:

```json
{
  "mcpServers": {
    "oxidex": { "command": "/path/to/oxidex-mcp", "args": [] }
  }
}
```

## Example conversation

> **User:** What camera took the photos in `~/Photos/2025/`, and which ones are missing a copyright tag?

The assistant calls `extract_metadata` with the glob, summarises `EXIF:Make` and
`EXIF:Model`, then calls `search_metadata` for files where `Copyright` is
absent. A follow-up "add `Copyright: 2025 Jane Doe` to those" runs
`write_metadata` with dry-run first and then for real once you confirm.

## Troubleshooting

- **Server not starting:** run the binary directly; it must print nothing and
  wait for JSON-RPC on stdin. A missing shared library or a wrong path in the
  client config is the usual cause.
- **Tools not visible:** send `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}`
  on stdin and check the five tools above are listed; then check the client's
  MCP log.
- **Permission errors:** the server only touches local files the launching
  user can access; path arguments are validated and `..` sequences refused.

## Resources

- **Source and issues:** [swack-tools/oxidex-mcp](https://github.com/swack-tools/oxidex-mcp)
- **MCP specification:** [spec.modelcontextprotocol.io](https://spec.modelcontextprotocol.io/)

## Related Documentation

- [Getting Started](/guide/getting-started) - install the OxiDex CLI and library
- [CLI Usage](/guide/cli-usage) - command-line reference
- [Library API](/guide/library-api) - Rust API
- [Supported Formats](/reference/formats/) - format list
