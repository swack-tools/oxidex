# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

Create fuzzing harnesses in fuzz/fuzz_targets/ for PDF and MP4 parsers. Set up continuous fuzzing: (1) Create fuzz_pdf.rs calling PDF parser with fuzzer-generated input, (2) Create fuzz_mp4.rs calling MP4 parser, (3) Seed corpus with sample valid files, (4) Configure cargo-fuzz to run both targets, (5) Document fuzzing process in README. Optionally submit to OSS-Fuzz for continuous fuzzing infrastructure.

---

## Issues Detected

*   **Critical Bug:** The PDF parser crashes when fuzzing due to multiple integer overflow panics in `src/parsers/pdf/info_parser.rs`. The fuzzer found crash files in `fuzz/artifacts/fuzz_pdf/crash-*.` When running the fuzzer with crash inputs, it panics with: `core::panicking::panic_const::panic_const_add_overflow`.

*   **Integer Overflow on Line 78:** The expression `file_size - tail_size as u64` can underflow when `tail_size` is larger than `file_size`. This was already partially fixed with `saturating_sub` on line 85, but line 78 was missed.

*   **Integer Overflow on Line 138:** The expression `startxref_pos + 9` can overflow. This is used to slice the string after the "startxref" keyword.

*   **Integer Overflow on Line 245:** The expression `start_num as u32 + i as u32` can overflow when parsing xref table entries.

*   **Integer Overflow on Line 296:** The expression `dict_start + 2` and `dict_start + dict_end` can overflow when extracting dictionary content.

*   **Testing Required:** After fixing, you MUST verify the fix works by running:
    ```bash
    cargo +nightly fuzz run fuzz_pdf fuzz/artifacts/fuzz_pdf/crash-5cac8bd701003e9fefe59920820f0709175f73c1
    cargo +nightly fuzz run fuzz_pdf -- -max_total_time=60
    ```
    Both commands must complete without crashes.

---

## Best Approach to Fix

You MUST modify `src/parsers/pdf/info_parser.rs` to use saturating arithmetic and proper bounds checking for ALL arithmetic operations that could overflow:

1. **Line 78**: Change `file_size - tail_size as u64` to `file_size.saturating_sub(tail_size as u64)`

2. **Line 138**: Change `&tail_str[startxref_pos + 9..]` to use `checked_add` and return an error if overflow:
   ```rust
   let after_start = startxref_pos.checked_add(9)
       .ok_or_else(|| ExifToolError::parse_error("Offset overflow after startxref"))?;
   if after_start > tail_str.len() {
       return Err(ExifToolError::parse_error("Invalid startxref position"));
   }
   let after_keyword = &tail_str[after_start..];
   ```

3. **Line 245**: Change `start_num as u32 + i as u32` to use `checked_add`:
   ```rust
   let obj_num = (start_num as u32).checked_add(i as u32)
       .ok_or_else(|| ExifToolError::parse_error("Object number overflow in xref table"))?;
   ```

4. **Line 296**: Change both additions to use `checked_add` and validate bounds:
   ```rust
   let content_start = dict_start.checked_add(2)
       .ok_or_else(|| ExifToolError::parse_error("Dictionary offset overflow"))?;
   let content_end = dict_start.checked_add(dict_end)
       .ok_or_else(|| ExifToolError::parse_error("Dictionary end offset overflow"))?;
   if content_end > input_str.len() {
       return Err(ExifToolError::parse_error("Dictionary extends beyond input"));
   }
   let dict_content = &input_str[content_start..content_end];
   ```

5. **IMPORTANT:** After making these changes, you MUST test the fix by:
   - Building the fuzzer: `cargo +nightly fuzz build fuzz_pdf`
   - Running with crash files: `cargo +nightly fuzz run fuzz_pdf fuzz/artifacts/fuzz_pdf/crash-5cac8bd701003e9fefe59920820f0709175f73c1`
   - Running for 1 minute: `cargo +nightly fuzz run fuzz_pdf -- -max_total_time=60`

   All commands must complete without panics or crashes. The fuzzer is expected to find parse errors (which is fine), but it must NOT panic or crash.

6. **Verify acceptance criteria:** After fixing the crashes, ensure:
   - `cargo +nightly fuzz build fuzz_pdf` compiles successfully
   - `cargo +nightly fuzz build fuzz_mp4` compiles successfully
   - Fuzzing runs for at least 1 minute without crashes
   - Corpus contains at least 3 valid samples each (already done)
   - README documents how to run fuzzing (already done)
