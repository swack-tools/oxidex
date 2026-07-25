# MCP Server Integration

OxiDex provides an MCP (Model Context Protocol) server that enables AI assistants like Claude to interact with file metadata through natural language conversations.

## What is MCP?

The Model Context Protocol (MCP) is an open standard that allows AI assistants to access external tools and data sources. The OxiDex MCP server exposes powerful metadata operations through 9 specialized tools, making it easy to extract, search, analyze, and modify file metadata without memorizing command-line syntax.

## Features

### 9 Specialized Tools

1. **extract_metadata** - Extract metadata from files
   - Works with single files or glob patterns (`*.jpg`, `photos/**/*.raw`)
   - Returns comprehensive metadata in human-readable format
   - Supports 300+ file formats

2. **write_metadata** - Write or update metadata tags
   - Batch operations with glob pattern support
   - Dry-run mode to preview changes before applying
   - Safe by default with confirmation prompts

3. **search_metadata** - Find files by metadata criteria
   - Filter by any metadata field with comparison operators
   - Supports `=`, `>`, `<`, `~` (contains) operators
   - Returns matching files with relevant metadata

4. **analyze_metadata** - Statistical analysis of metadata
   - Aggregate metadata across multiple files
   - Identify most common values and patterns
   - Calculate file size and distribution statistics

5. **copy_metadata** - Copy metadata between files
   - Transfer metadata from one file to multiple destinations
   - Dry-run preview mode for safety
   - Batch copying with glob pattern support

6. **detect_format** - Detect file format and capabilities
   - Identify file format using magic byte detection
   - Show supported metadata operations for each format
   - Works even with incorrect file extensions

7. **list_tags** - Browse available metadata tags
   - Explore 32,677+ metadata tags supported by OxiDex
   - Filter by group (EXIF, XMP, IPTC, GPS, etc.)
   - Filter by file format or search by keyword
   - Show only writable tags for write operations

8. **get_tag_info** - Get detailed tag information
   - View comprehensive tag descriptions
   - See data types, formats, and usage examples
   - Understand tag relationships and dependencies

9. **list_tag_groups** - Explore metadata tag groups
   - Browse metadata organized by standard groups
   - See tag counts per group and format support
   - Filter by file format to see format-specific groups

## Installation

### Prerequisites

- Rust 1.75+ (for building from source)
- Or download pre-built binaries from [releases](https://github.com/swack-tools/oxidex/releases)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/swack-tools/oxidex.git
cd oxidex

# Build the MCP server
cargo build --release -p oxidex-mcp

# The binary will be at: ./target/release/oxidex-mcp
```

### System Installation

```bash
# Install to system path
cargo install --path oxidex-mcp

# Or copy binary to a permanent location
sudo cp target/release/oxidex-mcp /usr/local/bin/
```

### Pre-built Binaries

Download the latest release for your platform:

- **macOS (Intel)**: `oxidex-mcp-darwin-x86_64`
- **macOS (Apple Silicon)**: `oxidex-mcp-darwin-aarch64`
- **Linux (x86_64)**: `oxidex-mcp-linux-x86_64`
- **Linux (ARM64)**: `oxidex-mcp-linux-aarch64`
- **Windows (x86_64)**: `oxidex-mcp-windows-x86_64.exe`

Make the binary executable (macOS/Linux):
```bash
chmod +x oxidex-mcp
sudo mv oxidex-mcp /usr/local/bin/
```

## Configuration

### Claude Desktop

Claude Desktop is Anthropic's official desktop application for Claude AI.

#### macOS Configuration

Edit: `~/Library/Application Support/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "oxidex": {
      "command": "/usr/local/bin/oxidex-mcp",
      "args": [],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

#### Windows Configuration

Edit: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "oxidex": {
      "command": "C:\\Program Files\\oxidex\\oxidex-mcp.exe",
      "args": [],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

#### Linux Configuration

Edit: `~/.config/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "oxidex": {
      "command": "/usr/local/bin/oxidex-mcp",
      "args": [],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

After configuring, restart Claude Desktop for changes to take effect.

### Claude Code (CLI)

Claude Code is Anthropic's command-line interface for Claude AI.

Edit: `~/.config/claude-code/mcp.json` (or your configured MCP settings file)

```json
{
  "mcpServers": {
    "oxidex": {
      "command": "/usr/local/bin/oxidex-mcp",
      "args": []
    }
  }
}
```

Restart Claude Code or start a new session to activate the MCP server.

### Cline (VS Code Extension)

Cline is a popular VS Code extension that supports MCP.

1. Open VS Code Settings
2. Search for "Cline MCP"
3. Add to MCP configuration:

```json
{
  "mcpServers": {
    "oxidex": {
      "command": "/usr/local/bin/oxidex-mcp",
      "args": []
    }
  }
}
```

### Other MCP Clients

The OxiDex MCP server implements the standard MCP protocol and works with any compatible client:

- **Continue**: Add to `~/.continue/config.json`
- **Zed Editor**: Add to Zed's MCP configuration
- **Custom Applications**: Use any MCP client library

Configuration format:
```json
{
  "mcpServers": {
    "oxidex": {
      "command": "/path/to/oxidex-mcp",
      "args": []
    }
  }
}
```

## Usage Examples

### Extracting Metadata

**Ask:** *"What camera took this photo?"*

```
User: What camera was used for photo.jpg?
Claude: [calls extract_metadata]
Claude: This photo was taken with a Canon EOS R5 on January 15, 2024 at 2:30 PM.
The lens used was an RF 24-70mm f/2.8L at 50mm, f/4.0, 1/500s, ISO 800.
```

**Ask:** *"Show me all metadata for JPEGs in this folder"*

```
User: Extract metadata from all JPEGs
Claude: [calls extract_metadata with "*.jpg"]
Claude: Found 15 JPEG files:

photo1.jpg:
  Make: Canon
  Model: EOS R5
  DateTimeOriginal: 2024-01-15 14:30:00
  GPS: 37.7749°N, 122.4194°W

photo2.jpg:
  Make: Sony
  Model: A7R IV
  DateTimeOriginal: 2024-01-20 09:15:00
  ...
```

### Writing Metadata

**Request:** *"Add copyright to all my photos"*

```
User: Add my copyright notice to all JPEGs
Claude: I'll update the copyright for all JPEG files. Let me first show you what
would change.
Claude: [calls write_metadata with dry_run=true]
Claude: This would update 15 files:
  photo1.jpg: Copyright → "© 2024 Your Name"
  photo2.jpg: Copyright → "© 2024 Your Name"
  ...

Should I proceed with these changes?

User: Yes, go ahead
Claude: [calls write_metadata]
Claude: Done! Updated copyright information in 15 JPEG files.
```

### Searching Files

**Query:** *"Find photos from my Canon camera last month"*

```
User: Find all Canon photos from October 2024
Claude: [calls search_metadata with filters for Make=Canon and date range]
Claude: Found 23 photos taken with Canon cameras in October 2024:

1. DSC_0123.jpg - Canon EOS R5 (Oct 5, 2024)
2. DSC_0124.jpg - Canon EOS R5 (Oct 8, 2024)
3. DSC_0125.jpg - Canon EOS R6 (Oct 12, 2024)
...

Would you like to see more details about any of these?
```

### Analyzing Metadata

**Request:** *"What cameras are in my photo collection?"*

```
User: Analyze camera usage in my photos
Claude: [calls analyze_metadata on photo directory]
Claude: Analyzed 150 photos from your collection:

Cameras Used:
  Canon (85 photos - 57%)
    - EOS R5: 60 photos
    - EOS R6: 25 photos
  Sony (50 photos - 33%)
    - A7R IV: 35 photos
    - A7 III: 15 photos
  iPhone (15 photos - 10%)
    - iPhone 14 Pro: 15 photos

Date Range: 2024-01-01 to 2024-11-15
GPS Tagged: 135/150 photos (90%)
Average File Size: 8.2 MB
```

### Copying Metadata

**Request:** *"Apply template metadata to new photos"*

```
User: Copy metadata from template.jpg to all photos in the new-batch folder
Claude: [calls copy_metadata with dry_run=true]
Claude: This would copy the following metadata from template.jpg to 24 files:
  - Artist: "Your Name"
  - Copyright: "© 2024 Your Name"
  - Keywords: ["Portfolio", "Professional"]
  - Contact Info: email@example.com

Should I proceed?

User: Yes
Claude: [calls copy_metadata]
Claude: Successfully copied metadata to 24 files in new-batch/
```

### Detecting File Formats

**Ask:** *"What format is this file and what can I do with it?"*

```
User: What format is mystery-file.dat?
Claude: [calls detect_format]
Claude: This is a JPEG image file (despite the .dat extension).

Supported Operations:
  ✓ Read EXIF metadata
  ✓ Read XMP metadata
  ✓ Read IPTC metadata
  ✓ Write EXIF tags
  ✓ Write XMP tags
  ✓ Read GPS coordinates
  ✓ Read camera maker notes

This format supports 2,847 metadata tags across 15 tag groups.
```

### Exploring Available Tags

**Ask:** *"What GPS tags can I read from photos?"*

```
User: Show me all GPS-related metadata tags
Claude: [calls list_tags with group="GPS"]
Claude: Found 45 GPS metadata tags:

Common GPS Tags:
  - GPS:GPSLatitude - Latitude coordinate
  - GPS:GPSLongitude - Longitude coordinate
  - GPS:GPSAltitude - Altitude above sea level
  - GPS:GPSDateStamp - GPS date (UTC)
  - GPS:GPSTimeStamp - GPS time (UTC)
  - GPS:GPSSpeed - Speed of movement
  - GPS:GPSTrack - Direction of movement

Extended GPS Tags:
  - GPS:GPSMapDatum - Geodetic survey data
  - GPS:GPSImgDirection - Direction of image
  - GPS:GPSDestLatitude - Destination latitude
  ...

Would you like details about any specific tag?
```

### Getting Tag Information

**Ask:** *"Tell me about the DateTimeOriginal tag"*

```
User: What is the DateTimeOriginal tag?
Claude: [calls get_tag_info for "EXIF:DateTimeOriginal"]
Claude: EXIF:DateTimeOriginal

Description:
The date and time when the original image was captured. This is the most
reliable timestamp for photos as it's set by the camera at capture time and
generally not modified by image editing software.

Data Type: String (DateTime)
Format: "YYYY:MM:DD HH:MM:SS"
Writable: Yes
Group: EXIF

Example Values:
  - "2024:11:15 14:30:25"
  - "2024:01:01 00:00:00"

Related Tags:
  - EXIF:DateTimeDigitized - When image was digitized
  - EXIF:DateTime - When file was modified
  - XMP:DateCreated - XMP creation date
```

### Listing Tag Groups

**Ask:** *"What metadata groups are available for PDFs?"*

```
User: Show metadata groups for PDF files
Claude: [calls list_tag_groups with format="PDF"]
Claude: PDF files support 8 metadata tag groups:

1. PDF (Core PDF metadata)
   - 127 tags
   - Document info, page layout, encryption details

2. XMP (Extensible Metadata Platform)
   - 856 tags
   - Dublin Core, Rights Management, Adobe schemas

3. EXIF (Image metadata)
   - 312 tags
   - Image properties, color space

4. IPTC (News/Editorial metadata)
   - 148 tags
   - Copyright, creator info, keywords

5. Photoshop (Adobe Photoshop metadata)
   - 94 tags
   - Layers, slices, history

...

Total: 1,847 tags available for PDF files
```

## Tool Reference

### extract_metadata

Extract metadata from one or more files.

**Parameters:**
- `path` (required): File path or glob pattern (e.g., `*.jpg`, `photos/**/*.raw`)

**Example:**
```json
{
  "name": "extract_metadata",
  "arguments": {
    "path": "photos/*.jpg"
  }
}
```

**Supports:** All 300+ file formats supported by OxiDex

---

### write_metadata

Write or update metadata tags.

**Parameters:**
- `path` (required): File path or glob pattern
- `tags` (required): Object with tag key-value pairs
- `dry_run` (optional): Preview changes without applying (default: false)

**Example:**
```json
{
  "name": "write_metadata",
  "arguments": {
    "path": "photo.jpg",
    "tags": {
      "Artist": "John Doe",
      "Copyright": "© 2024 John Doe",
      "Keywords": ["nature", "landscape"]
    },
    "dry_run": true
  }
}
```

---

### search_metadata

Search for files matching metadata criteria.

**Parameters:**
- `directory` (required): Directory to search
- `filters` (required): Array of filter expressions

**Filter Operators:**
- `=` - Equals (e.g., `Make=Canon`)
- `>` - Greater than (e.g., `FileSize>1000000`)
- `<` - Less than (e.g., `ISO<400`)
- `~` - Contains (e.g., `Model~R5`)

**Example:**
```json
{
  "name": "search_metadata",
  "arguments": {
    "directory": "photos",
    "filters": [
      "Make=Canon",
      "DateTimeOriginal>2024-01-01",
      "ISO<800"
    ]
  }
}
```

---

### analyze_metadata

Generate statistical summaries of metadata.

**Parameters:**
- `path` (required): File path or glob pattern

**Example:**
```json
{
  "name": "analyze_metadata",
  "arguments": {
    "path": "photos/*.jpg"
  }
}
```

Returns statistics including:
- Most common camera models
- Date ranges
- GPS coordinate coverage
- File size distribution
- Metadata completeness

---

### copy_metadata

Copy metadata from one file to others.

**Parameters:**
- `source` (required): Source file path
- `destination` (required): Destination file path or glob pattern
- `dry_run` (optional): Preview changes (default: false)

**Example:**
```json
{
  "name": "copy_metadata",
  "arguments": {
    "source": "template.jpg",
    "destination": "batch/*.jpg",
    "dry_run": true
  }
}
```

---

### detect_format

Detect file format and show supported operations.

**Parameters:**
- `path` (required): File path or glob pattern

**Example:**
```json
{
  "name": "detect_format",
  "arguments": {
    "path": "unknown-file.dat"
  }
}
```

Returns:
- Detected format name
- Supported read/write operations
- Available tag groups
- Tag count

---

### list_tags

List available metadata tags with optional filtering.

**Parameters:**
- `group` (optional): Filter by tag group (e.g., "EXIF", "XMP", "IPTC")
- `format` (optional): Filter by file format (e.g., "JPEG", "PNG")
- `writable` (optional): Show only writable tags (boolean)
- `search` (optional): Search tags by name or description

**Example:**
```json
{
  "name": "list_tags",
  "arguments": {
    "group": "GPS",
    "writable": true
  }
}
```

---

### get_tag_info

Get detailed information about a specific tag.

**Parameters:**
- `tag` (required): Tag name (e.g., "EXIF:Make", "XMP:Creator")

**Example:**
```json
{
  "name": "get_tag_info",
  "arguments": {
    "tag": "EXIF:DateTimeOriginal"
  }
}
```

Returns:
- Tag description
- Data type and format
- Whether tag is writable
- Example values
- Related tags

---

### list_tag_groups

List all metadata tag groups with counts and format support.

**Parameters:**
- `format` (optional): Filter by file format

**Example:**
```json
{
  "name": "list_tag_groups",
  "arguments": {
    "format": "JPEG"
  }
}
```

Returns groups like:
- EXIF (Exchangeable Image File Format)
- XMP (Extensible Metadata Platform)
- IPTC (International Press Telecommunications Council)
- GPS (Global Positioning System)
- MakerNotes (Camera manufacturer specific)

## Troubleshooting

### Server Not Starting

**Symptom:** MCP client shows "Server failed to start"

**Solutions:**
1. Verify the binary path is correct in your configuration file
2. Check binary has execute permissions:
   ```bash
   chmod +x /usr/local/bin/oxidex-mcp
   ```
3. Test manually:
   ```bash
   echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | oxidex-mcp
   ```
   Should return a JSON response with server info

4. Check for errors in system logs (macOS):
   ```bash
   log show --predicate 'process == "Claude"' --last 5m
   ```

### Tools Not Visible

**Symptom:** AI assistant doesn't see OxiDex tools

**Solutions:**
1. Restart your MCP client completely (quit and relaunch)
2. Verify JSON syntax in config file:
   ```bash
   python -m json.tool < ~/.config/Claude/claude_desktop_config.json
   ```
3. Check for conflicting server names in config
4. Enable debug logging (see below)

### Permission Denied Errors

**Symptom:** "Permission denied" when accessing files

**Solutions:**
1. Check file and directory permissions:
   ```bash
   ls -la /path/to/files
   ```
2. Ensure the user running the MCP client has read access
3. For write operations, verify write permissions
4. On macOS, grant Full Disk Access to Claude Desktop in System Settings

### Debug Mode

Enable detailed logging to diagnose issues:

```json
{
  "mcpServers": {
    "oxidex": {
      "command": "/usr/local/bin/oxidex-mcp",
      "env": {
        "RUST_LOG": "debug"
      }
    }
  }
}
```

Log levels:
- `error` - Only errors
- `warn` - Warnings and errors
- `info` - General information (default)
- `debug` - Detailed debugging information
- `trace` - Very verbose output

View logs:
- **macOS/Linux:** Check stderr in terminal if running manually
- **Claude Desktop:** Check Console.app (macOS) or system logs
- **Claude Code:** Logs appear in terminal output

### Common Issues

**Issue:** Glob patterns not working

**Solution:** Ensure patterns are quoted in natural language requests. The MCP server handles glob expansion internally.

**Issue:** Metadata not found

**Solution:** Verify the file contains metadata:
```bash
oxidex -a photo.jpg
```

**Issue:** Write operations failing

**Solution:**
1. Check file is not read-only
2. Verify format supports writing (use `detect_format` tool)
3. Try dry-run mode first to see what would change

## Performance

The OxiDex MCP server inherits the performance benefits of the core OxiDex library:

- **Fast Startup:** Server launches in <100ms
- **Low Overhead:** Minimal performance impact over direct library usage
- **Parallel Processing:** Batch operations use multiple cores
- **Efficient Streaming:** Processes large file sets without loading all into memory

## Security

The MCP server includes several security features:

- **Path Validation:** Blocks directory traversal attacks (`..` sequences)
- **Input Sanitization:** All inputs validated before processing
- **Dry-Run Mode:** Preview destructive operations before applying
- **Local Only:** No network operations, only local file access
- **Safe Defaults:** Write operations require explicit confirmation

## Architecture

The MCP server uses a modular architecture:

```
oxidex-mcp/
├── src/
│   ├── main.rs           # Entry point, stdio handling
│   ├── server.rs         # MCP protocol implementation
│   ├── format.rs         # Format detection
│   ├── utils.rs          # Path validation, glob expansion
│   └── tools/
│       ├── mod.rs        # Tool registry and dispatcher
│       ├── extract.rs    # Metadata extraction
│       ├── write.rs      # Metadata writing
│       ├── search.rs     # Metadata search
│       ├── analyze.rs    # Statistical analysis
│       ├── copy.rs       # Metadata copying
│       ├── detect_format.rs  # Format detection
│       ├── list_tags.rs      # Tag listing
│       ├── tag_info.rs       # Tag information
│       └── tag_groups.rs     # Tag group listing
└── tests/                # Comprehensive test suite
```

**Protocol Flow:**
1. Client sends JSON-RPC request via stdin
2. Server parses and validates request
3. Request routed to appropriate handler
4. Handler processes with OxiDex library
5. Results formatted as human-readable text
6. Response sent to stdout as JSON-RPC

## Development

### Running Tests

```bash
# Run all tests
cargo test -p oxidex-mcp

# Run specific test suite
cargo test -p oxidex-mcp --test integration_tests

# Run with output
cargo test -p oxidex-mcp -- --nocapture
```

### Manual Testing

Test the server directly:

```bash
# Test initialization
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | \
  cargo run -p oxidex-mcp --quiet

# List available tools
echo '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | \
  cargo run -p oxidex-mcp --quiet

# Extract metadata
echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"extract_metadata","arguments":{"path":"photo.jpg"}}}' | \
  cargo run -p oxidex-mcp --quiet
```

### Contributing

Contributions to the MCP server are welcome! Areas where help is needed:

- Additional tool implementations
- Enhanced metadata analysis features
- Performance optimizations
- Documentation improvements
- Testing with different MCP clients
- Integration examples

See the main [contributing guide](https://github.com/swack-tools/oxidex/blob/main/CONTRIBUTING.md) for details.

## License

The OxiDex MCP server is part of the OxiDex project and is licensed under GPL-3.0.

## Resources

- **Source Code:** [oxidex-mcp/](https://github.com/swack-tools/oxidex/tree/main/oxidex-mcp) in main repository
- **MCP Specification:** [spec.modelcontextprotocol.io](https://spec.modelcontextprotocol.io/)
- **Issue Tracker:** [GitHub Issues](https://github.com/swack-tools/oxidex/issues)
- **Discussions:** [GitHub Discussions](https://github.com/swack-tools/oxidex/discussions)

## Related Documentation

- [Getting Started](/guide/getting-started) - Install OxiDex CLI and library
- [CLI Usage](/guide/cli-usage) - Command-line interface reference
- [Library API](/guide/library-api) - Rust API documentation
- [Supported Formats](/reference/formats/) - Complete format list
