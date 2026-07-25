# CLAUDE.md Redundancy Removal Design

**Date:** 2025-12-03
**Phase:** 1 - Remove redundancy and reorganize content
**Goal:** Eliminate duplication between root and oxidex CLAUDE.md files using inheritance model

## Problem

Two CLAUDE.md files with redundant content:
- Root: `/Users/allen/Documents/git/CLAUDE.md` (general configuration)
- Project: `/Users/allen/Documents/git/oxidex/CLAUDE.md` (project-specific)

**Redundancies identified:**
- Cargo commands in both files
- Style rules duplicated (files under 500 lines, cargo clippy)
- Build/test commands overlap

## Solution: Inheritance Model

**Root CLAUDE.md** = Language-agnostic Claude Code operating system
**OxiDex CLAUDE.md** = Project-specific overrides and additions only

### Root CLAUDE.md - What Stays

1. **Core batching rules** - Fundamental Claude Code behavior
2. **Agent execution patterns** - Task tool usage
3. **Agent coordination hooks** - MCP tool coordination
4. **Frequently used agents** - Comprehensive reference (all agent types)
5. **Important reminders** - Universal principles

### Root CLAUDE.md - What Changes

1. **Common Commands** → Remove entirely (move to project files)
2. **Code Style** → Keep only universal rules:
   - Files under 500 lines
   - Never hardcode secrets
   - Write tests before implementation
   - Language-specific linting moves to projects
3. **File Organization** → Keep as-is for Phase 1 (rewrite in Phase 2)

### OxiDex CLAUDE.md - What Stays

1. **Overview** - Rust ExifTool implementation context
2. **Commands** - All Rust/Cargo/Just commands
3. **Structure** - Project directory layout
4. **Architecture** - Hexagonal architecture details
5. **Style** - Rust-specific: cargo clippy, cargo fmt, plus OxiDex conventions

### OxiDex CLAUDE.md - What Removes

1. "Files under 500 lines" - Moves to root as universal rule
2. Any duplicate reminders already in root

### Agent List Strategy (Split)

- **Root:** Comprehensive agent reference with all types
- **Project:** Optional "Top 5 for this project" if needed

## Implementation Steps

1. Update root CLAUDE.md:
   - Remove "Common Commands" section (npm/cargo commands)
   - Update "Code Style" to remove `cargo clippy` specifics
   - Keep comprehensive agent list

2. Update oxidex CLAUDE.md:
   - Remove "Files under 500 lines" from Style section
   - Keep all Rust/Cargo specific content
   - Ensure no other duplicates with root

3. Test configuration with simple task

4. Commit changes

## Phase 2 (Future)

- Rewrite file organization rules as principles rather than prescriptive folders
- Further refinement based on usage patterns

## Success Criteria

- No content duplication between files
- Root file is language-agnostic and reusable
- OxiDex file contains only project-specific knowledge
- Configuration works correctly for development tasks
