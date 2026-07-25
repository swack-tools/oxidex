# Office Document Forensic Metadata Extraction

This document describes the enhanced forensic metadata extraction capabilities for Office Open XML (OOXML) documents.

## Overview

The OOXML parser has been enhanced to extract hidden and forensic metadata from Microsoft Office documents (DOCX, XLSX, PPTX) for document forensics and investigation purposes.

## Supported Document Types

- DOCX (Microsoft Word)
- XLSX (Microsoft Excel)
- PPTX (Microsoft PowerPoint)

## Extracted Metadata

### Core Properties (docProps/core.xml)

These properties follow the Dublin Core metadata standard:

| Tag Name | XML Element | Description |
|----------|-------------|-------------|
| `OOXML:Title` | `dc:title` | Document title |
| `OOXML:Creator` | `dc:creator` | Document creator |
| `OOXML:Subject` | `dc:subject` | Document subject |
| `OOXML:Description` | `dc:description` | Document description |
| `OOXML:CreateDate` | `dcterms:created` | Creation timestamp |
| `OOXML:ModifyDate` | `dcterms:modified` | Last modified timestamp |
| `OOXML:LastModifiedBy` | `cp:lastModifiedBy` | Last person to modify document |
| `OOXML:RevisionNumber` | `cp:revision` | Document revision count |
| `OOXML:LastPrinted` | `cp:lastPrinted` | When document was last printed |
| `OOXML:Category` | `cp:category` | Document category |
| `OOXML:ContentStatus` | `cp:contentStatus` | Status (Draft, Final, etc.) |

### Application Properties (docProps/app.xml)

Application-specific metadata:

| Tag Name | XML Element | Description |
|----------|-------------|-------------|
| `OOXML:Application` | `Application` | Office application name |
| `OOXML:AppVersion` | `AppVersion` | Application version |
| `OOXML:Company` | `Company` | Organization name |
| `OOXML:Manager` | `Manager` | Manager name |
| `OOXML:Template` | `Template` | Template file used |
| `OOXML:TotalEditTime` | `TotalTime` | Total editing time (formatted) |
| `OOXML:HyperlinkBase` | `HyperlinkBase` | Base URL for hyperlinks |
| `OOXML:DocSecurity` | `DocSecurity` | Document security level |
| `OOXML:Pages` | `Pages` | Number of pages |
| `OOXML:Words` | `Words` | Word count |
| `OOXML:Characters` | `Characters` | Character count |

### PowerPoint-Specific Properties

| Tag Name | XML Element | Description |
|----------|-------------|-------------|
| `OOXML:HiddenSlides` | `HiddenSlides` | Count of hidden slides |
| `OOXML:PresentationFormat` | `PresentationFormat` | Presentation format |

### Custom Properties (docProps/custom.xml)

User-defined metadata:

| Tag Name Format | Description |
|----------------|-------------|
| `OOXML:Custom:<PropertyName>` | Custom properties defined by users |

Examples:
- `OOXML:Custom:ProjectID`
- `OOXML:Custom:Classification`
- `OOXML:Custom:ReviewCount`

## Special Features

### Human-Readable Edit Time

The `TotalTime` field (stored in minutes) is automatically converted to human-readable format:

- Input: `45` (minutes)
- Output: `"45 minutes"`

- Input: `90` (minutes)
- Output: `"1 hour 30 minutes"`

- Input: `150` (minutes)
- Output: `"2 hours 30 minutes"`

## Forensic Use Cases

### 1. Authorship Investigation

Extract creator and last modified by information:
```
OOXML:Creator = "John Doe"
OOXML:LastModifiedBy = "Jane Smith"
```

### 2. Timeline Analysis

Track document creation and modification history:
```
OOXML:CreateDate = "2024-01-15T10:30:00Z"
OOXML:ModifyDate = "2024-01-20T15:45:00Z"
OOXML:LastPrinted = "2024-01-18T09:00:00Z"
```

### 3. Corporate Attribution

Identify organizational information:
```
OOXML:Company = "Acme Corp"
OOXML:Manager = "Bob Johnson"
```

### 4. Document Provenance

Determine template and application used:
```
OOXML:Template = "Normal.dotm"
OOXML:Application = "Microsoft Office Word"
OOXML:AppVersion = "16.0000"
```

### 5. Edit Activity Analysis

Track editing effort:
```
OOXML:RevisionNumber = "42"
OOXML:TotalEditTime = "2 hours 30 minutes"
```

### 6. Hidden Content Detection

Identify hidden slides in presentations:
```
OOXML:HiddenSlides = "3"
```

### 7. Custom Metadata Extraction

Extract user-defined properties for specialized workflows:
```
OOXML:Custom:ProjectID = "PROJ-12345"
OOXML:Custom:Classification = "Internal Use Only"
OOXML:Custom:ReviewCount = "5"
```

## Implementation Details

### File Structure

The implementation is located in:
- **Parser**: `/src/parsers/document/ooxml.rs`
- **Tests**: `/tests/unit/document/ooxml_tests.rs`

### Functions

1. `parse_core_properties(xml: &str, metadata: &mut MetadataMap) -> Result<()>`
   - Parses Dublin Core metadata from core.xml
   - Extracts forensic fields like LastModifiedBy, Revision, LastPrinted

2. `parse_app_properties(xml: &str, metadata: &mut MetadataMap) -> Result<()>`
   - Parses application properties from app.xml
   - Converts TotalTime to human-readable format
   - Extracts Company, Manager, Template, etc.

3. `parse_custom_properties(xml: &str, metadata: &mut MetadataMap) -> Result<()>`
   - Parses user-defined custom properties from custom.xml
   - Handles various value types (string, integer, boolean)

4. `format_edit_time(minutes: u64) -> String`
   - Converts minutes to human-readable time format
   - Handles singular/plural forms correctly

### XML Parsing

The parser uses `quick-xml` for efficient XML parsing and handles:
- Namespaced elements (cp:, dc:, dcterms:, vt:)
- Self-closing tags
- Attribute extraction
- UTF-8 encoding

## Testing

Comprehensive unit tests verify:

1. **Core Properties Parsing**
   - Basic Dublin Core metadata
   - Forensic fields (LastModifiedBy, Revision, Category, ContentStatus)

2. **Application Properties Parsing**
   - Standard properties (Application, Company, Manager)
   - Edit time formatting
   - PowerPoint-specific properties (HiddenSlides)

3. **Custom Properties Parsing**
   - User-defined properties
   - Various data types (string, integer)

4. **Time Formatting**
   - Edge cases (0 minutes, 1 minute, 1 hour)
   - Singular/plural forms
   - Hour + minute combinations

Run tests:
```bash
cargo test --lib ooxml::tests
```

## Example Output

For a typical DOCX file with forensic metadata:

```
OOXML:Title = "Confidential Report"
OOXML:Creator = "John Doe"
OOXML:Subject = "Q4 Analysis"
OOXML:CreateDate = "2024-01-15T10:30:00Z"
OOXML:ModifyDate = "2024-01-20T15:45:00Z"
OOXML:LastModifiedBy = "Jane Smith"
OOXML:RevisionNumber = "42"
OOXML:LastPrinted = "2024-01-18T09:00:00Z"
OOXML:Category = "Financial"
OOXML:ContentStatus = "Final"
OOXML:Application = "Microsoft Office Word"
OOXML:AppVersion = "16.0000"
OOXML:Company = "Acme Corp"
OOXML:Manager = "Bob Johnson"
OOXML:Template = "Corporate.dotx"
OOXML:TotalEditTime = "2 hours 30 minutes"
OOXML:Pages = "15"
OOXML:Words = "3542"
OOXML:Custom:ProjectID = "PROJ-12345"
OOXML:Custom:Classification = "Internal Use Only"
```

## OOXML Structure

Office documents are ZIP archives containing:

```
docProps/
  ├── app.xml       - Application properties
  ├── core.xml      - Core Dublin Core properties
  └── custom.xml    - Custom user properties (optional)
word/               - Document content (DOCX)
xl/                 - Workbook content (XLSX)
ppt/                - Presentation content (PPTX)
```

## Security Considerations

When performing forensic analysis:

1. **Metadata Privacy**: Be aware that metadata may contain sensitive personal information
2. **Tampering Detection**: Suspicious values (e.g., RevisionNumber = 1 with extensive edits) may indicate metadata manipulation
3. **Timezone Analysis**: Timestamps may reveal user location
4. **Software Versions**: Application versions can identify potential vulnerabilities

## Future Enhancements

Potential improvements:

1. **Metadata Validation**
   - Flag suspicious revision counts
   - Detect timestamp inconsistencies
   - Identify metadata tampering

2. **Extended Analysis**
   - Parse document.xml for embedded objects
   - Extract relationship mappings
   - Analyze document structure changes

3. **Derived Metrics**
   - Calculate edit velocity (revisions per hour)
   - Identify working patterns from timestamps
   - Cross-reference author information

4. **Reporting**
   - Generate forensic summary reports
   - Highlight anomalies
   - Create timeline visualizations
