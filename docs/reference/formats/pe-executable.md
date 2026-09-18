# Executables: PE, ELF and Mach-O

OxiDex reads Windows PE (EXE, DLL, SYS), ELF and Mach-O files. Their tags
are reported in the `EXE` group, as in ExifTool.

## Parity with ExifTool

Measured with a release build of the `refactor/tag-machinery` tip against the
pinned ExifTool 13.59 (`-j -G1`), on the three executables in ExifTool's own
test corpus:

| File | ExifTool tags | Matched exactly | Missing | OxiDex-only tags |
| --- | ---: | ---: | ---: | ---: |
| `t/images/EXE.exe` (PE32) | 34 | 34 | 0 | 24 |
| `t/images/EXE.elf` | 7 | 7 | 0 | 40 |
| `t/images/EXE.macho` | 9 | 9 | 0 | 30 |

Every tag ExifTool prints for these files comes out of OxiDex with the same
name and value. That includes the version resource of a PE file:
`CompanyName`, `FileVersion`, `ProductName` and the rest.

::: warning OxiDex-only tags are printed by default
OxiDex also emits analysis tags of its own in the `EXE` group, and ExifTool
has no such tags. For PE, these are `ASLR`, `DEP`, `ControlFlowGuard`,
`ImportedDLLs`, `ImportedFunctions`, `HasSuspiciousImports`, `CompileTime`,
`ImageBaseHex` and others. ELF and Mach-O get section, segment, symbol and
hardening summaries. `--extended-output` does not gate these tags today, so
they appear in default output, and the conformance measurements count them
as EXTRA. Do not expect them from ExifTool.
:::

## Detection

- **PE**: `MZ` at offset 0, then `PE\0\0` at the offset in `e_lfanew`.
  PE32 and PE32+ are both handled.
- **ELF**: `\x7fELF`.
- **Mach-O**: the Mach-O magic numbers. A Mach-O static
  library (`.a`) is identified, but returns only its identity tags.

## Rich header

The undocumented Visual Studio Rich header in PE files is decoded
separately. See [PE Rich header](/features/pe-rich-header).

## Example

```bash
oxidex -j -G1 program.exe
oxidex -EXE:MachineType -EXE:TimeStamp -EXE:FileVersion program.exe
```
