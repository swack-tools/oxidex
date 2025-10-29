# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I1.T5",
  "iteration_id": "I1",
  "iteration_goal": "Establish project foundation with directory structure, build system, core domain models, architectural diagrams, and basic JPEG EXIF parsing capability to validate end-to-end workflow.",
  "description": "Create JSON Schema defining the structure for TagDescriptor objects that will be code-generated from ExifTool documentation. Schema should define: tag_id (string or number), tag_name (string), format_family (enum: EXIF, XMP, IPTC, GPS, etc.), writable (boolean), value_type (enum: String, Integer, Rational, Binary, etc.), description (string), example_values (array of strings). Save to api/tag_database_schema.json. Validate against JSON Schema Draft 7 specification.",
  "agent_type_hint": "BackendAgent",
  "inputs": "Section 2 (Data Model Overview), Section 2.1 (Key Architectural Artifacts)",
  "target_files": [
    "api/tag_database_schema.json"
  ],
  "input_files": [],
  "deliverables": "Valid JSON Schema file",
  "acceptance_criteria": "JSON Schema validates against Draft 7 spec (use online validator or ajv CLI), schema includes all fields mentioned in task description, schema has appropriate constraints (e.g., tag_name is required, writable is boolean), example valid TagDescriptor object passes schema validation",
  "dependencies": [
    "I1.T1"
  ],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: Data Model Overview (from 03_System_Structure_and_Data.md)

```markdown
### 3.6. Data Model Overview & ERD

**Description**: ExifTool-RS operates on files without persistent database storage. The "data model" represents in-memory structures for metadata representation. The Entity-Relationship Diagram below models the logical relationships between metadata concepts.

#### Key Entities

1. **File**: Represents a media file being processed (JPEG, PNG, etc.)
2. **MetadataMap**: Collection of all metadata tags extracted from a file
3. **TagValue**: A single metadata tag with its name, value, and type information
4. **TagDescriptor**: Definition of a tag (from tag database) including ID, name, type constraints, format family
5. **FormatFamily**: Grouping of related metadata standards (EXIF, XMP, IPTC, MakerNotes)
6. **IFD (Image File Directory)**: TIFF-specific structural element containing tags
```

### Context: TagDescriptor Entity Details (from 03_System_Structure_and_Data.md)

```markdown
entity TagDescriptor {
  primary_key(tag_name) : String
  --
  tag_id : u16 | String
  foreign_key(family_id) : String
  writable : bool
  value_type : TagType
  description : String
  example_values : Vec<String>
}

entity FormatFamily {
  primary_key(family_id) : String
  --
  family_name : String
  specification_url : String
}
```

**Rationale**:

- **No Persistent Database**: The system is stateless. `MetadataMap` exists only in-memory during processing and is serialized to JSON/text output or written back to file metadata.

- **Variant Value Type**: `TagValue.value` uses a Rust `enum` to represent heterogeneous tag types:
  ```rust
  enum TagValueData {
      String(String),
      Number(f64),
      Integer(i64),
      Binary(Vec<u8>),
      Rational { numerator: i32, denominator: i32 },
      Struct(HashMap<String, TagValueData>), // For complex XMP structures
  }
  ```

- **Tag Descriptor**: Compile-time generated from ExifTool tag database. In practice, this is a large static `HashMap<&'static str, TagDescriptor>` embedded in the binary, not a runtime database.

### Context: Data Model Overview (from 01_Plan_Overview_and_Setup.md)

```markdown
*   **Data Model Overview:**
    *   **File:** Represents media file being processed (path, format, size)
    *   **MetadataMap:** Collection of all tags extracted from a file
    *   **TagValue:** Single metadata tag with name, value, type information, and optional byte offset
    *   **TagDescriptor:** Tag definition from database (ID, name, type constraints, format family)
    *   **FormatFamily:** Grouping of metadata standards (EXIF, XMP, IPTC, MakerNotes)
    *   **IFD (Image File Directory):** TIFF-specific structural element for tag organization

    **Note:** No persistent database storage. All data structures are in-memory during processing, serialized to JSON/text output or written back to file metadata.
```

### Context: Task I1.T5 Specification (from 02_Iteration_I1.md)

```markdown
*   **Task 1.5: Define Tag Database Schema**
    *   **Task ID:** `I1.T5`
    *   **Description:** Create JSON Schema defining the structure for TagDescriptor objects that will be code-generated from ExifTool documentation. Schema should define: tag_id (string or number), tag_name (string), format_family (enum: EXIF, XMP, IPTC, GPS, etc.), writable (boolean), value_type (enum: String, Integer, Rational, Binary, etc.), description (string), example_values (array of strings). Save to `api/tag_database_schema.json`. Validate against JSON Schema Draft 7 specification.
    *   **Agent Type Hint:** `BackendAgent` or `DocumentationAgent`
    *   **Inputs:** Section 2 (Data Model Overview), Section 2.1 (Key Architectural Artifacts)
    *   **Input Files:** []
    *   **Target Files:**
        *   `api/tag_database_schema.json`
    *   **Deliverables:**
        *   Valid JSON Schema file
    *   **Acceptance Criteria:**
        *   JSON Schema validates against Draft 7 spec (use online validator or `ajv` CLI)
        *   Schema includes all fields mentioned in task description
        *   Schema has appropriate constraints (e.g., tag_name is required, writable is boolean)
        *   Example valid TagDescriptor object passes schema validation
    *   **Dependencies:** `I1.T1`
    *   **Parallelizable:** Yes (can run concurrently with T2, T3, T4, T6 after T1 completes)
```

### Context: Tag Registry Component (from 03_System_Structure_and_Data.md)

```markdown
Component(tag_registry, "Tag Registry", "Generated const maps", "28K+ tag definitions indexed by ID/name")
Component(validation, "Validation Engine", "Rust", "Tag value type checking, range validation")
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `Cargo.toml`
    *   **Summary:** This is the project manifest for the Rust workspace. It defines the package metadata, dependencies, and build profiles.
    *   **Current State:** The project is properly initialized with all required dependencies (serde, serde_json, clap, nom, quick-xml, chrono, encoding_rs, memmap2, rayon) already specified. The build system is configured for both library and binary targets.
    *   **Recommendation:** You DO NOT need to modify this file. The dependencies required for JSON schema work (serde, serde_json) are already present.

*   **File:** `src/core/tag_descriptor.rs`
    *   **Summary:** This file is a placeholder stub with only comments and an allow(dead_code) directive. It currently contains NO actual implementation.
    *   **Current State:** Empty stub file with module documentation only.
    *   **Recommendation:** This file will be implemented in task I1.T6 (next task after I1.T5). The JSON schema you create in this task MUST accurately reflect the Rust structure that will be implemented in tag_descriptor.rs during I1.T6.

*   **File:** `src/core/metadata_map.rs`
    *   **Summary:** This file is a placeholder stub with only comments and an allow(dead_code) directive. It currently contains NO actual implementation.
    *   **Current State:** Empty stub file with module documentation only.
    *   **Recommendation:** This file will be implemented in I1.T6. Your schema focuses on TagDescriptor, not MetadataMap, so this file is NOT directly relevant to your current task.

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** This file is a placeholder stub with only comments. It currently contains NO actual TagValue enum implementation.
    *   **Current State:** Empty stub file with module documentation only.
    *   **Recommendation:** This file will be implemented in I1.T6. The value_type field in your JSON schema MUST match the TagValue enum variants that will be defined in this file (String, Integer, Float, Rational, Binary, DateTime, Struct).

*   **File:** `src/error/mod.rs`
    *   **Summary:** This file is a placeholder stub with only comments. It currently contains NO actual ExifToolError enum implementation.
    *   **Current State:** Empty stub file with module documentation only.
    *   **Recommendation:** This file is NOT relevant to your current task. You are defining the TagDescriptor schema, which does not involve error types.

*   **Directory:** `api/`
    *   **Summary:** This directory exists but is currently empty.
    *   **Current State:** The directory was created during project initialization (I1.T1) but contains no files yet.
    *   **Recommendation:** You MUST create `api/tag_database_schema.json` in this directory. This is the primary deliverable for your task.

### Implementation Tips & Notes

*   **Tip:** According to the architecture, the `tag_id` field can be EITHER a `u16` (numeric ID like 0x010F for EXIF Make) OR a `String` (named ID like "XMP-dc:Creator"). Your JSON schema MUST support BOTH types. Use a `oneOf` constraint with two sub-schemas (one for integer, one for string).

*   **Tip:** The `format_family` field should be constrained to a specific set of values. Based on the architecture, the valid enum values are: `"EXIF"`, `"XMP"`, `"IPTC"`, `"GPS"`, `"ICC_Profile"`, `"Photoshop"`, `"MakerNotes"`, `"JFIF"`, `"PNG"`, `"PDF"`, `"QuickTime"`. You SHOULD define this as an enum constraint in the schema.

*   **Tip:** The `value_type` field represents the TagValue enum variants from the architecture. The valid values based on the ERD and architecture are: `"String"`, `"Integer"`, `"Float"`, `"Rational"`, `"Binary"`, `"DateTime"`, `"Struct"`. You MUST constrain this field to these exact values using an enum.

*   **Note:** The schema you create will be used in TWO ways:
    1. **Immediate (I1.T6):** As documentation for implementing the Rust TagDescriptor struct in the next task
    2. **Future (I5.T5):** As a validation schema for the build.rs script that will auto-generate tag definitions from ExifTool source

*   **Warning:** The acceptance criteria explicitly require validation against JSON Schema Draft 7 specification. You MUST include `"$schema": "http://json-schema.org/draft-07/schema#"` as the first field in your schema. This ensures compatibility with standard validators.

*   **Tip:** Per the acceptance criteria, you SHOULD include an example valid TagDescriptor object that passes schema validation. Consider adding this as documentation in a comment or in a separate `examples` section within the schema itself (though not strictly required by JSON Schema spec).

*   **Note:** Based on the ERD diagram from the architecture, the TagDescriptor entity has the following field characteristics:
    - `tag_name`: PRIMARY KEY (required, string, unique identifier)
    - `tag_id`: Can be u16 or String (required)
    - `format_family`: Foreign key to FormatFamily (required, string from enum)
    - `writable`: Boolean (required, indicates if tag can be written)
    - `value_type`: Enum of TagType (required)
    - `description`: String (required, human-readable description)
    - `example_values`: Array of strings (required)

*   **Tip:** All fields mentioned in the task description are REQUIRED fields. None are optional. Your schema constraints should reflect this with appropriate `required` array specification.

*   **Warning:** The schema will be consumed by Rust code generation tools in iteration I5. Ensure field names use snake_case (Rust convention) NOT camelCase (JavaScript convention). Use: `tag_id`, `tag_name`, `format_family`, `value_type`, `example_values` (NOT `tagId`, `tagName`, etc.).

### Validation Strategy

*   **Strategy:** After creating the schema file, you MUST validate it. The acceptance criteria mention two validation approaches:
    1. **Online validator:** Use a service like https://www.jsonschemavalidator.net/ with Draft 7 selected
    2. **CLI validator:** Use `ajv-cli` if available (`npm install -g ajv-cli`, then `ajv validate -s schema.json -d data.json`)

*   **Testing:** Create at least one example TagDescriptor JSON object that conforms to your schema. Example based on architecture:
    ```json
    {
      "tag_id": 271,
      "tag_name": "EXIF:Make",
      "format_family": "EXIF",
      "writable": true,
      "value_type": "String",
      "description": "Manufacturer of the recording equipment",
      "example_values": ["Canon", "Nikon", "Sony"]
    }
    ```
    This example should successfully validate against your schema.

### Project Context

*   **Completed Tasks:** Tasks I1.T1 (project initialization), I1.T2 (component diagram), I1.T3 (ERD), and I1.T4 (sequence diagram) are complete. The project structure, dependencies, and architectural documentation are all in place.

*   **Next Task:** After you complete I1.T5, the next task will be I1.T6 (Implement Core Domain Models), which will create the actual Rust structs based on the schema you define. Your schema MUST be accurate and complete to enable smooth implementation in I1.T6.

*   **Directory Structure:** The `api/` directory is the correct location for API specifications and schemas. The `docs/` directory contains diagrams and documentation. The `src/` directory contains Rust source code. Keep these concerns separated.
