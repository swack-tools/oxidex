# Step 10 design checkpoint — a bypass-proof `DecodedField` API

**Status: SIGNED OFF by the maintainer 2026-08-11. Implementation authorized.**

Decisions as answered:
- **D1 — withhold on all six reasons at once** (not staged).
- **D2 — `RawAccess::new` requires a static `PerlCitation`.**
- **D3 — omit-and-count by default**; hand-supply semantics via `RawAccess` + oracle
  test only where the Perl condition is simple enough to implement faithfully in the
  same step. The full list of dropped tags is reported either way, so the maintainer can
  overrule per tag.
- **D4 — prove the bypass-proof property with a `trybuild` dev-dependency**, compile-fail
  cases as real `.rs` fixtures with expected stderr (NOT the `compile_fail` doctest I
  originally recommended, and not privacy alone).

Authored by the Stage 2 orchestrator (never delegated, per plan rule 7).
Base: `refactor/tag-machinery` @ `6dd5f0c2`. Depends on Step 9 (in flight) for the
`hook` / `subdirectory` / `offsets_sound_until` flags this design consumes.

---

## 1. What is actually broken

`Field::omitted` records three refusal reasons today (`binary_tables.rs:103-113`):
`value_conv`, `raw_conv`, `condition`. Step 9 adds `hook`, `subdirectory`, and per-table
`offsets_sound_until`.

Exactly **one** of those flags is consulted anywhere in the runtime. `DecodedField`
(`runtime.rs:62-88`) is two public fields and one guarded method:

```rust
pub struct DecodedField {
    pub field: &'static Field,   // pub
    pub raw:   DecodedValue,     // pub  <-- the hole
}

impl DecodedField {
    pub fn apply_print_conv_to_raw(&self) -> Option<String> {
        if self.field.omitted.value_conv { return None; }   // the only check
        apply_print_conv(self.field.print_conv, &self.raw)
    }
}
```

The guarded method is correct and its doc comment is honest. The problem is that
`raw` is `pub`, so the guard is advisory: any caller can read `decoded.raw` and do its
own conversion, and the compiler is happy. Step 4 found a live instance — the Sony
`BatteryVoltage` arm hand-converted `decoded.raw` on a field whose `omitted.condition`
was set, and shipped `384.02 V` for a tag ExifTool does not emit at all.

Current consumers of `.raw`, from `rg` at the tip — **9 files, 17 sites**:

| File | sites |
|---|---|
| `src/parsers/tiff/makernotes/sony/amount.rs` | 6 |
| `src/parsers/quicktime/metadata_extractor.rs` | 4 |
| `src/parsers/image/photocd.rs` | (many, via `field.raw`) |
| `src/core/jpeg_helpers.rs` | 1 |
| `src/parsers/image/dpx.rs` | 1 |
| `src/parsers/canon_vrd/ver2.rs` | 1 |
| `src/parsers/tiff/makernotes/samsung/stmn.rs` | 1 |
| `src/parsers/audio/dss.rs` | 1 |
| `src/parsers/jpeg/app_parsers.rs` | 1 |
| `src/parsers/raw/metadata.rs` | 1 |

Step 4's audit checked each against its table's flags and found none currently gated —
so **the hazard is latent, not live**, and this step is about making it structurally
impossible before a future caller makes it live.

---

## 2. The design

### 2.1 Two-stage decode: refuse in `decode_binary_table`, then gate the accessor

`decode_binary_table` gains a refusal pass. A field is withheld from the returned
`Vec<DecodedField>` when any of `condition`, `raw_conv`, `value_conv`, `hook`,
`subdirectory` is set, or when its offset is at/after the table's `offsets_sound_until`.
Withheld fields are **counted, not dropped** — the function's return type changes:

```rust
pub struct TableDecode {
    fields:   Vec<DecodedField>,   // private
    refusals: RefusalCounts,       // private
}

pub struct RefusalCounts {
    pub condition: usize,
    pub raw_conv: usize,
    pub value_conv: usize,
    pub hook: usize,
    pub subdirectory: usize,
    pub unsound_offset: usize,
}
```

`TableDecode` exposes `fields()`, `refusals()`, and `into_parts()`. The counts are what
Step 13's diagnostic sink consumes so a decode-time refusal is countable per file
(the plan's ledger artifact 4).

### 2.2 `raw` stops being `pub`; `emit()` is the only value accessor

```rust
pub struct DecodedField {
    pub field: &'static Field,
    raw: DecodedValue,            // private
}

impl DecodedField {
    /// The value ExifTool would report, or None when a semantic is unresolved.
    pub fn emit(&self) -> Option<TagValue> { ... }   // consults ALL flags
}
```

`emit()` subsumes `apply_print_conv_to_raw`, which is removed. Because
`decode_binary_table` already withheld the flagged fields, `emit()` returning `None`
after that is a belt-and-braces case (short raw bytes, unrepresentable value), but the
check stays so the type is safe in isolation.

### 2.3 The escape hatch is a type, and it carries an obligation

Some decodes genuinely need the raw value plus caller-supplied semantics — Step 4's
NEX orientation is the live example: `0x0016` is a two-alternative model-conditioned
Perl array the generator does not transcribe, so the condition can only be supplied by
hand until Step 23 makes variants data.

```rust
/// Raw access for a caller that supplies the missing semantics itself.
///
/// Construction requires naming the flags you are overriding and the Perl
/// definition you are implementing. Every construction site must have an
/// oracle-backed test; `RawAccess::new` is `#[track_caller]` and the
/// staleness suite (Step 16) enumerates the sites.
pub struct RawAccess<'a> {
    field: &'a DecodedField,
}

impl<'a> RawAccess<'a> {
    pub fn new(
        field: &'a DecodedField,
        acknowledged: Acknowledged,   // must cover every flag actually set
        justification: &'static PerlCitation,
    ) -> Option<Self>;

    pub fn raw(&self) -> &DecodedValue;
}
```

`Acknowledged` is a bitflags-style struct, not a bool: a caller that acknowledges
`condition` but not `value_conv` still gets `None` if both are set. `PerlCitation` is
`{ module, table, tag, lines }` — a `&'static` const the call site defines, which is
also exactly the record Step 16's staleness suite needs to diff against the dump.

### 2.4 Why not just make `raw` private and add a getter

Because a plain `raw()` getter is the same hole with an extra keystroke. The point is
that reaching the raw value should require the caller to *state what it is taking
responsibility for*, in a form that is greppable and testable. `RawAccess` makes the
override appear in `rg 'RawAccess::new'` as a finite, auditable list — which is the
plan's "no production caller reads `decoded.raw` outside the opt-in type" exit criterion,
made mechanical rather than aspirational.

---

## 3. What this costs

All 17 sites must be triaged into one of three outcomes:

1. **`emit()`** — the common case; the field has no unresolved semantics.
2. **`RawAccess`** — the field is flagged and the caller supplies the semantics, with a
   Perl citation and an oracle test. Expected: the Sony NEX orientation read, likely a
   few PhotoCD sites.
3. **Deleted** — the caller was reading a flagged field and should not have been.
   Expected: none, per Step 4's audit, but the triage is what proves it.

This is a mechanical but broad refactor. My estimate is that the type changes are half a
day and the 17-site triage is the real work, because each `RawAccess` site needs an
oracle-backed test written against a real sample.

**Risk I want to flag:** `decode_binary_table` withholding fields is a *behavioral*
change, not just a typing one. Any field currently emitted whose `condition` or
`raw_conv` flag is set will stop being emitted. That is correct by the project's
omit-and-count rule — but it will move conformance numbers, and some of those tags may
currently be *accidentally right* (exactly like Step 4's `CameraOrientation`, which read
a coincidentally-matching byte). I plan to measure the delta before and after with Step
12's group-qualified instrument and report it, rather than assume it is all improvement.

---

## 4. Decisions I need from you

**D1 — Scope of the withholding.** Withhold on all six reasons at once, or stage it
(`condition` + `raw_conv` first, `hook` + `subdirectory` + `unsound_offset` after Step 9
settles)? Staging is safer and gives two smaller conformance deltas to read; one shot is
faster. **My recommendation: all six at once**, because Step 9 lands first anyway and a
half-enforced API invites exactly the bypass this step exists to close.

**D2 — `RawAccess` strictness.** Should `RawAccess::new` require a `PerlCitation`, as
designed above? It is friction, and it is friction on purpose — but it is friction on
every legitimate hand implementation too. **My recommendation: yes, require it**, since
Step 16 needs the citation anyway and collecting it at construction is free compared to
reconstructing it later.

**D3 — What happens to accidentally-right tags.** If the conformance delta shows a tag
that was passing and now disappears (because it is flagged), do I (a) accept the loss and
count it, per omit-and-count, or (b) treat each one as a mini-Step-4, hand-supply the
semantics via `RawAccess` with an oracle test, and keep the tag? **My recommendation:
(a) by default, (b) only where the Perl condition is simple enough to implement
faithfully in the same step** — with the full list reported either way, so you can
overrule per tag. This is the decision most likely to change the shape of the work, which
is why I am not guessing at it.

**D4 — Compile-fail test.** The plan asks for "a compile-fail/doc test demonstrating raw
access requires the opt-in type". That means adding a `trybuild`-style dev-dependency, or
a `compile_fail` doctest. **My recommendation: a `compile_fail` doctest**, no new
dependency.

---

## 5. What happens after sign-off

Step 10 implements the above, then Step 11 (fractional indices at `floor(index)`) becomes
unblocked — it depends on Step 10 so that newly-decodable fields still respect the other
flags. Steps 12 and 13 are already in flight and do not depend on this checkpoint.
