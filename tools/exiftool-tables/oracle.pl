#!/usr/bin/env perl
#
# Emit ExifTool binary-table facts as flat TSV, straight from the live Perl
# hashes.  This is the ground truth the generated Rust is checked against.
#
# It deliberately shares NO code with dump_tables.pl.  A verifier that reuses
# the extractor's own JSON would only prove the codegen is self-consistent --
# it would happily confirm a bug that both sides inherited.  Going back to
# ExifTool independently is what makes a disagreement meaningful.
#
# Output columns:
#   MODULE  TABLE  INDEX  NAME                      -- one per field (4 columns)
#   MODULE  TABLE  INDEX  ENUM    KEY  VALUE        -- one per PrintConv entry (6)
#   MODULE  TABLE  INDEX  MASK    BITS  SHIFT       -- one per masked field (6)
#   MODULE  TABLE  INDEX  HOOK    (empty)           -- field carries a Hook (5)
#   MODULE  TABLE  INDEX  VARFMT  (empty)           -- field's Format is var_* (5)
#   MODULE  TABLE  INDEX  BITMASK BIT  LABEL        -- one per BITMASK sub-hash entry (6)
#   MODULE  TABLE  INDEX  OTHER   PRINTHEX          -- field's PrintConv has OTHER (5)
#   MODULE  TABLE  INDEX  FORMAT  SPELLING          -- field's raw Format (5)
#   MODULE  TABLE  INDEX  RAWCONV  1                -- field has RawConv (5)
#   MODULE  TABLE  INDEX  VALUECONV 1               -- field has ValueConv (5)
#   MODULE  TABLE  INDEX  CONDITION 1               -- field has Condition (5)
#   MODULE  TABLE  INDEX  PRINTCONV 1               -- field has PrintConv (5)
#   MODULE  TABLE  INDEX  UNKNOWN 1                 -- field is Unknown (5)
#   MODULE  TABLE  INDEX  MASKDECL 1                -- field declares Mask (5)
#   MODULE  TABLE  ''     TGROUPS G0  G1  G2        -- table's raw GROUPS (7)
#   MODULE  TABLE  INDEX  GROUPS  G0  G1  G2        -- tag's own Groups (7)
#   MODULE  TABLE  INDEX  SUBDIR  TAGTABLE  START  BASE  PROCESSPROC  BYTEORDER  VALIDATE
#                                                    -- field carries a SubDirectory (10)
#   KEYED MODULE TABLE  INDEX  NAME NAME             -- ProcessCanonRaw keyed row (6)
#   KEYED MODULE TABLE  INDEX  <property> ...        -- keyed-row facts, with
#                                                    -- the same property spellings as
#                                                    -- the binary rows above
#   KEYED MODULE TABLE  ''     TGROUPS G0 G1 G2       -- raw table groups (8)
#   KEYED MODULE TABLE  INDEX  COUNT N                -- raw Count (6)
#   KEYED MODULE TABLE  INDEX  FLAGS U B L P A PRIORITY -- shared reporting policy (11)
#   KEYED MODULE TABLE  INDEX  GROUPS G0 G1 G2        -- raw tag groups (8)
#   KEYED MODULE TABLE  INDEX  SUBDIR TAGTABLE START VALIDATE PROCESSPROC (9)
#   NATIVE_PROCESSOR MODULE TABLE JSON-FACT -- every table-owned PROCESS_PROC,
#                                               emitted after all modules load;
#                                               separate from KEYED scope
#   NATIVE_PROCESSOR_TABLE MODULE TABLE JSON-FACT -- table identity/properties,
#                                               complete special-tag metadata,
#                                               including zero-row tables
#   NATIVE_PROCESSOR_ROW MODULE TABLE RAW_ID VARIANT JSON-FACT -- one record
#                                               per plain entry or array alternative;
#                                               VARIANT is '-' or zero-based decimal
#
# The trailing empty column on HOOK/VARFMT lines is not decorative: it is
# what keeps them from colliding with a NAME line on column count (both
# would otherwise be 4 columns, and a tag genuinely named "Hook" is not
# impossible). BITMASK/OTHER are Step 25's addition, read straight from the
# live `PrintConv` hash: BITMASK emits one row per `$pc->{BITMASK}` entry
# (same shape as ENUM, so `verify.py` can reuse its pair-comparison logic);
# OTHER is presence-only (a tag's `OTHER` closure is arbitrary Perl this
# oracle does not evaluate -- see `tools/exiftool-tables/others.py` for how
# `codegen.py` decides which ones it trusts), with PRINTHEX carrying
# `$e->{PrintHex}`'s presence so `verify.py` can confirm a generated
# `PartialEnumInt`'s `print_hex` flag against the SAME tag-level fact
# `codegen.py`'s `conv_for` reads. SUBDIR carries the raw facts Step 27's
# `verify.py` needs to
# independently re-derive whether `codegen.py`'s SubdirEdge compiler
# (`tools/exiftool-tables/subdirs.py`) should have modeled this field's edge
# or refused it, and why -- TAGTABLE/START/BASE are the raw (to_text) source
# strings, empty when the key is absent; PROCESSPROC/BYTEORDER/VALIDATE are
# '1' when the key is present at all (a coderef for ProcessProc, an arbitrary
# scalar or expression for the other two) and '' when absent, since presence
# alone is what codegen.py's compiler gates on for these three (see
# src/exiftool_tables/subdir.rs's module doc for why: ProcessProc changes how
# the target is walked, and ByteOrder/Validate are keys ProcessBinaryData's
# SubDirectory branch never reads at all).
#
# Slice I-1 (IFD-style tables, docs/superpowers/specs/2026-09-06-ifd-tables-
# design.md section 4): a SECOND scope, emitted from the same walk, for the
# tables ExifTool reads with ProcessExif -- PROCESS_PROC absent, or naming
# `Image::ExifTool::Exif::ProcessExif`, on a hash that is a tag table by
# ExifTool's own `%specialTags` (a table-level key, or a structured tag
# entry). Every IFD row carries a leading kind column `IFD` so the two row
# spaces cannot collide even for the two tables that fall in BOTH scopes
# (a scalar FORMAT with no PROCESS_PROC); the binary rows above are emitted
# exactly as before, byte for byte. A module named `IFD` would defeat that
# routing, so the walk dies if it ever meets one.
#
#   IFD MODULE TABLE ''   TGROUPS   G0 G1 G2                 -- raw GROUPS (8)
#   IFD MODULE TABLE ''   SETGROUP1 VALUE                    -- SET_GROUP1 verbatim (6)
#   IFD MODULE TABLE ''   PRIORITY  N                        -- table PRIORITY verbatim (6)
#   IFD MODULE TABLE KEY  NAME      NAME                     -- one per tag entry (6)
#   IFD MODULE TABLE KEY  FORMAT    SPELLING                 -- tag's raw Format (6)
#   IFD MODULE TABLE KEY  COUNT     N                        -- tag's raw Count (6)
#   IFD MODULE TABLE KEY  WRITABLE  SPELLING                 -- Writable when it is a
#                                                               format spelling, never 0/1 (6)
#   IFD MODULE TABLE KEY  ENUM      KEY VALUE                -- one per PrintConv entry (7)
#   IFD MODULE TABLE KEY  BITMASK   BIT LABEL                -- one per BITMASK entry (7)
#   IFD MODULE TABLE KEY  OTHER     PRINTHEX                 -- PrintConv has OTHER (6)
#   IFD MODULE TABLE KEY  PCREF     REFTYPE                  -- PrintConv is a non-HASH ref (6)
#   IFD MODULE TABLE KEY  PCEXPR    1                        -- PrintConv is a scalar (Perl expression) (6)
#   IFD MODULE TABLE KEY  GROUPS    G0 G1 G2                 -- tag's own Groups (8)
#   IFD MODULE TABLE KEY  FLAGS     UNKNOWN BINARY LIST PROTECTED AVOID PRIORITY
#                                                            -- 0/1 each, PRIORITY a
#                                                               number or '-' (11)
#   IFD MODULE TABLE KEY  RAWCONV   TEXT                     -- whitespace-collapsed RawConv,
#                                                               '__CODE__' for a code ref (6)
#   IFD MODULE TABLE KEY  HOOK      1                        -- tag carries a Hook (6)
#   IFD MODULE TABLE KEY  CONDITION 1                        -- a PLAIN entry carries a
#                                                               Condition (6)
#   IFD MODULE TABLE KEY  SUBDIR    TAGTABLE START BASE PROCESSPROC BYTEORDER VALIDATE
#                                   FIXFORMAT SUBIFD MAXSUBDIRS DIRNAME       (15)
#   IFD MODULE TABLE KEY  VALIDATION EXPRESSION CALLEE SOURCE_FILE SOURCE_SHA256 JSON-HELPER (10)
#                                   -- scalar SubDirectory Validate provenance/body fact
#   IFD MODULE TABLE KEY  PROCESSOR TARGET_MODULE TARGET_TABLE ORIGIN JSON-FACT (9)
#                                   -- final effective child processor after all modules load
#
# KEY is the integer tag id as ExifTool keys it, or `"$k#$i"` for the i-th
# alternative of a `_variants` arrayref (same convention as the binary rows).
# FLAGS reads each flag the way ExifTool's `ExpandFlags` (ExifTool.pm:5878-
# 5894) would materialise it -- a direct `Binary => 1` and a `Flags =>
# ['Unknown','Binary']` are the same fact -- because `SetupTagTable` has not
# necessarily run on a hash this walk reads raw. SUBDIR's text facts are the
# raw (to_text) source strings, '-' when the key is absent, `__REF__` for a
# reference; PROCESSPROC is the resolved sub name (the generator emits an
# edge through one only when it names ProcessBinaryData); VALIDATE is 1/0
# presence; FIXFORMAT is the TAG's raw FixFormat; SUBIFD is 1 when the tag
# has the SubIFD flag or `FixFormat => 'ifd'` (Exif.pm's SubIFD loop reads
# the pointer(s) rather than the bytes). CONDITION is emitted only for a
# plain entry: an alternative's Condition is what selects it, compiled by
# conds.py into the `Cond` that guards it, so it is not an omission there.

use strict;
use warnings;
use Encode qw(decode);
use B ();
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use File::Spec ();
use FindBin;
use Scalar::Util qw(refaddr);
use lib $FindBin::Bin;
use JSON::PP ();
use OxiDex::NativeReaderContract ();

my $LIB = shift @ARGV or die "usage: $0 <exiftool-lib-dir>\n";
unshift @INC, $LIB;
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
require Image::ExifTool;
binmode(STDOUT, ':encoding(UTF-8)');

sub txt {
    my ($s) = @_;
    return '' unless defined $s;
    return $s if utf8::is_utf8($s);
    my $d = eval { decode('UTF-8', $s, Encode::FB_CROAK) };
    return defined $d ? $d : decode('ISO-8859-1', $s);
}

sub clean { my $s = txt($_[0]); $s =~ s/[\t\n\r]+/ /g; return $s }

# Source-file provenance comes from the live callee CODE ref, not the dump
# or generated audit record. A changed helper invalidates a stale artifact
# even when its owning module's tag tables did not change.
sub keyed_validation_helper_fact {
    my ($expression) = @_;
    return processor_unresolved_fact('', 'validation_expression_unavailable')
        unless defined $expression && !ref $expression
            && $expression =~ /^\s*((?:[A-Za-z_]\w*::)+[A-Za-z_]\w*)\s*\(/;
    my $callee = $1;
    no strict 'refs';
    my $cv = *{$callee}{CODE};
    return processor_code_ref_fact($cv, $callee);
}

sub keyed_validation_source {
    my ($expression) = @_;
    return ('', '', '') unless defined $expression && !ref $expression;
    return ('', '', '') unless $expression =~ /^\s*((?:[A-Za-z_]\w*::)+[A-Za-z_]\w*)\s*\(/;
    my $callee = $1;
    no strict 'refs';
    my $cv = *{$callee}{CODE};
    return ($callee, '', '') unless $cv;
    my $file = eval { B::svref_2object($cv)->FILE };
    my $root = abs_path($LIB);
    my $path = defined $file ? abs_path($file) : undef;
    return ($callee, '', '') unless defined $path && defined $root && index($path, "$root/") == 0;
    open(my $fh, '<:raw', $path) or return ($callee, '', '');
    local $/;
    my $bytes = <$fh>;
    close($fh) or return ($callee, '', '');
    return ($callee, substr($path, length($root) + 1), sha256_hex($bytes));
}

# Emit every row for one tag-info entry `$e` at `$key`. Shared between a
# plain (scalar-keyed) entry and one alternative of a Step 23 `_variants`
# array -- the two carry identical fields (Name/Mask/Hook/SubDirectory/
# Format/PrintConv), only how `$key` was built differs (see the caller).
sub emit_entry {
    my ($mod, $sym, $key, $e) = @_;
    my $name = ref $e eq 'HASH' ? $e->{Name} : $e;
    return unless defined $name && !ref $name;
    print join("\t", $mod, $sym, $key, clean($name)), "\n";

    return unless ref $e eq 'HASH';

    # Mask/BitShift decide what the field's value even is: ExifTool
    # reduces the word to ($val & Mask) >> BitShift before converting.
    # BitShift is derived here the way ExifTool derives it -- lowest set
    # bit, unless the table states one -- rather than read from
    # dump_tables.pl's JSON, so the two paths stay independent.
    my $mask = $e->{Mask};
    if (defined $mask && !ref $mask && $mask) {
        my $shift = $e->{BitShift};
        unless (defined $shift) {
            $shift = 0;
            ++$shift until $mask & (1 << $shift);
        }
        print join("\t", $mod, $sym, $key, 'MASK', $mask, $shift), "\n";
    }

    # Hook and SubDirectory are the two constructs codegen.py records
    # but cannot execute (see tools/exiftool-tables/codegen.py's
    # `omitted_for`). A Hook can rewrite later fields' format/byte order
    # in ways this generator does not run, so presence alone is all a
    # caller needs. A SubDirectory means the bytes are the entry to a
    # nested table -- Step 27 additionally models WHERE that entry leads
    # (src/exiftool_tables/subdir.rs), so its row carries the raw facts
    # (independently of dump_tables.pl/codegen.py/subdirs.py) that decide
    # whether that modeling should have succeeded.
    print join("\t", $mod, $sym, $key, 'HOOK', ''), "\n" if defined $e->{Hook};
    if (defined $e->{SubDirectory} && ref $e->{SubDirectory} eq 'HASH') {
        my $sd = $e->{SubDirectory};
        my $rawtext = sub {
            my ($v) = @_;
            return '' unless defined $v;
            return ref $v ? '__REF__' : clean($v);
        };
        my $present = sub { defined $_[0] ? '1' : '' };
        print join("\t", $mod, $sym, $key, 'SUBDIR',
            $rawtext->($sd->{TagTable}),
            $rawtext->($sd->{Start}),
            $rawtext->($sd->{Base}),
            $present->($sd->{ProcessProc}),
            $present->($sd->{ByteOrder}),
            $present->($sd->{Validate}),
        ), "\n";
    }

    # A `var_*` Format is data-dependent width: ExifTool computes the
    # real byte offset by walking the bytes, so the generator's static
    # `index * increment` formula is unsound for every field at or
    # past this one (`offsets_sound_until`).
    my $fmt = $e->{Format};
    print join("\t", $mod, $sym, $key, 'VARFMT', ''), "\n"
        if defined $fmt && !ref $fmt && $fmt =~ /^var_/;

    # A PrintConv that is a REF but not a HASH is not an enum: it is a Perl
    # CODE ref (`PrintConv => \&SomeSub`) or, rarely, an ARRAY. `codegen.py`
    # translates only the CODE refs `exprs.py`'s CODE_REFS registry names and
    # REFUSES the rest, recording the refusal as `Omitted { print_conv: true }`
    # so the field is withheld rather than emitted carrying a raw number under
    # ExifTool's own tag name. Presence and the ref TYPE are all `verify.py`
    # needs to re-derive, independently of dump_tables.pl, which fields ought
    # to carry that flag.

    # Step 26: the field's raw Format spelling, so verify.py can check that
    # the Fmt variant in the generated Rust is the one ExifTool declares --
    # and, just as importantly, that a field ExifTool gives a Format the
    # generator does NOT support was refused rather than emitted at the
    # table's default width. Before this row existed, verify.py checked every
    # field's name, enum, mask, hook and subdirectory but never its width: a
    # format transcribed at the wrong size would have decoded neighbouring
    # bytes under a correct tag name and passed the whole suite.
    print join("\t", $mod, $sym, $key, 'FORMAT', clean($fmt)), "\n"
        if defined $fmt && !ref $fmt;

    # These are presence facts for the native-minus-generated inventory.  A
    # completely withheld field cannot carry `Omitted` in Rust, so the
    # generated omission sidecar names one or more native semantics that made
    # withholding necessary.  The verifier checks those names against these
    # rows; a sidecar cannot relabel an ordinary new tag as "unsupported".
    print join("\t", $mod, $sym, $key, 'RAWCONV', 1), "\n" if defined $e->{RawConv};
    print join("\t", $mod, $sym, $key, 'VALUECONV', 1), "\n" if defined $e->{ValueConv};
    print join("\t", $mod, $sym, $key, 'CONDITION', 1), "\n" if defined $e->{Condition};
    print join("\t", $mod, $sym, $key, 'PRINTCONV', 1), "\n" if defined $e->{PrintConv};
    print join("\t", $mod, $sym, $key, 'UNKNOWN', 1), "\n" if $e->{Unknown};
    print join("\t", $mod, $sym, $key, 'MASKDECL', 1), "\n" if defined $e->{Mask};

    # Step 26: the tag's OWN Groups overrides, families 0/1/2. Empty column
    # means the tag does not override that family -- which is not the same as
    # having no group; the table supplies it (ExifTool.pm:9236-9244).
    my $tgrp = $e->{Groups};
    if (ref $tgrp eq 'HASH') {
        print join("\t", $mod, $sym, $key, 'GROUPS',
            map { defined $tgrp->{$_} && !ref $tgrp->{$_} ? clean($tgrp->{$_}) : '' } (0, 1, 2)
        ), "\n";
    }

    my $pc = $e->{PrintConv};
    if (defined $pc && ref $pc && ref $pc ne 'HASH') {
        print join("\t", $mod, $sym, $key, 'PCREF', ref $pc), "\n";
    }
    return unless ref $pc eq 'HASH';

    # Step 25: the BITMASK sub-hash and OTHER closure's presence, straight
    # from the live PrintConv hash -- independent of dump_tables.pl/
    # codegen.py/others.py the same way every other row here is. `verify.py`
    # cross-checks these against the generated `PrintConv::Bitmask`/
    # `PartialEnumInt` fields: a BITMASK bit this generator invented or
    # dropped is exactly as wrong as an ENUM/MASK mismatch, and a
    # `PartialEnumInt` field whose tag has no OTHER at all in ExifTool would
    # mean the registry matched a closure that was never actually there.
    if (ref $pc->{BITMASK} eq 'HASH') {
        for my $bk (sort keys %{$pc->{BITMASK}}) {
            print join("\t", $mod, $sym, $key, 'BITMASK', clean($bk),
                       clean($pc->{BITMASK}{$bk})), "\n";
        }
    }
    if (exists $pc->{OTHER}) {
        my $print_hex = (defined $e->{PrintHex} && $e->{PrintHex}) ? '1' : '';
        print join("\t", $mod, $sym, $key, 'OTHER', $print_hex), "\n";
    }

    for my $ck (sort keys %$pc) {
        next if $ck =~ /^(BITMASK|OTHER|Notes|PrintHex|SeparateTable)$/;
        next if ref $pc->{$ck};
        print join("\t", $mod, $sym, $key, 'ENUM', clean($ck),
                   clean($pc->{$ck})), "\n";
    }
}

# ProcessCanonRaw uses keyed CIFF directory entries rather than BinaryTable's
# offset sequence.  Keep its oracle rows on their own leading kind: the keyed
# schema is independently parsed and must never be smuggled into the binary
# verifier's layout assumptions.  This stays deliberately small because the
# first keyed slice inventories source facts only; it does not execute a
# caller or a CIFF reader.
sub emit_keyed_entry {
    my ($mod, $sym, $key, $e, $table) = @_;
    # SetupTagTable expands once, before selection or output. Flags may
    # override any source property, including Name, Condition and Format.
    my $x = ref $e eq 'HASH' ? expanded_flags($e) : {};
    $e = { %$e, %$x } if ref $e eq 'HASH';
    my $name = ref $e eq 'HASH' ? $e->{Name} : $e;
    return unless defined $name && !ref $name;
    print join("\t", 'KEYED', $mod, $sym, $key, 'NAME', clean($name)), "\n";
    # Expand flags independently from the generator, using the same native
    # source facts already used by the IFD oracle below. Table AVOID wins
    # over per-tag Avoid; per-tag Priority wins over table PRIORITY.
    my $policy = ref $e eq 'HASH' ? $e : {};
    my $avoid = defined $table->{AVOID} ? $table->{AVOID} : flag_fact($policy, $x, 'Avoid');
    my $priority = flag_fact($policy, $x, 'Priority');
    $priority = $table->{PRIORITY} unless defined $priority;
    print join("\t", 'KEYED', $mod, $sym, $key, 'FLAGS',
        (map { flag_fact($policy, $x, $_) ? 1 : 0 } qw(Unknown Binary List Protected)),
        ($avoid ? 1 : 0), dash_text($priority),
    ), "\n";
    return unless ref $e eq 'HASH';

    my $fmt = $e->{Format};
    print join("\t", 'KEYED', $mod, $sym, $key, 'FORMAT', clean($fmt)), "\n"
        if defined $fmt && !ref $fmt;
    print join("\t", 'KEYED', $mod, $sym, $key, 'COUNT', clean($e->{Count})), "\n"
        if defined $e->{Count} && !ref $e->{Count};
    print join("\t", 'KEYED', $mod, $sym, $key, 'RAWCONV', 1), "\n" if defined $e->{RawConv};
    print join("\t", 'KEYED', $mod, $sym, $key, 'VALUECONV', 1), "\n" if defined $e->{ValueConv};
    print join("\t", 'KEYED', $mod, $sym, $key, 'CONDITION', clean($e->{Condition})), "\n" if defined $e->{Condition} && !ref $e->{Condition};
    print join("\t", 'KEYED', $mod, $sym, $key, 'PRINTCONV', 1), "\n" if defined $e->{PrintConv};
    print join("\t", 'KEYED', $mod, $sym, $key, 'UNKNOWN', 1), "\n" if flag_fact($e, $x, 'Unknown');
    print join("\t", 'KEYED', $mod, $sym, $key, 'MASKDECL', 1), "\n" if defined $e->{Mask};
    my $groups = $e->{Groups};
    if (ref $groups eq 'HASH') {
        print join("\t", 'KEYED', $mod, $sym, $key, 'GROUPS',
            map { defined $groups->{$_} && !ref $groups->{$_} ? clean($groups->{$_}) : '' } (0, 1, 2)
        ), "\n";
    }
    if (defined $e->{SubDirectory} && ref $e->{SubDirectory} eq 'HASH') {
        my $sd = $e->{SubDirectory};
        my $text = sub { defined $_[0] && !ref $_[0] ? clean($_[0]) : '' };
        print join("\t", 'KEYED', $mod, $sym, $key, 'SUBDIR',
            $text->($sd->{TagTable}), $text->($sd->{Start}),
            defined($sd->{Validate}) ? '1' : '', defined($sd->{ProcessProc}) ? '1' : '',
        ), "\n";
        if (defined $sd->{Validate} && !ref $sd->{Validate}) {
            print join("\t", 'KEYED', $mod, $sym, $key, 'VALIDATION',
                clean($sd->{Validate}), keyed_validation_source($sd->{Validate})), "\n";
        }
    }
    my $pc = $e->{PrintConv};
    if (ref $pc eq 'HASH') {
        for my $ck (sort keys %$pc) {
            next if $ck =~ /^(BITMASK|OTHER|Notes|PrintHex|SeparateTable)$/;
            next if ref $pc->{$ck};
            print join("\t", 'KEYED', $mod, $sym, $key, 'ENUM', clean($ck),
                       clean($pc->{$ck})), "\n";
        }
    }
}

# --- Slice I-1: the IFD-style scope ---------------------------------------

# Fully-qualified name of a CODE ref, '' when B cannot name it (an anonymous
# sub reports as `...::__ANON__`, which is fine: the only question ever asked
# of the answer is whether it ends in a specific named sub).
sub sub_name {
    my ($cv) = @_;
    my $b = eval { B::svref_2object($cv) };
    return '' unless $b && $b->isa('B::CV');
    my $gv = eval { $b->GV };
    return '' unless $gv && ref($gv) ne 'B::SPECIAL';
    return eval { $gv->STASH->NAME . '::' . $gv->NAME } // '';
}

# PROCESS_PROC is a table-owned CODE reference.  Capture that exact CV only
# after all modules load, while resolving bare calls through the final package
# glob they use at execution time.  This oracle deliberately duplicates the
# dumper's provenance work instead of importing it: a verifier needs an
# independently live source fact, not shared extractor behavior.
my $PROCESSOR_LIB_ABS = abs_path($LIB);
sub processor_unresolved_fact {
    my ($name, $reason) = @_;
    return {
        __perl => 'CODE', __opaque => JSON::PP::true, __name => $name,
        resolved => JSON::PP::false, __deparse => undef,
        source_file => undef, source_sha256 => undef, reason => $reason,
    };
}

sub processor_source_file_fact {
    my ($cv) = @_;
    my $file = eval { B::svref_2object($cv)->FILE };
    return (undef, undef, 'source_file_unavailable') unless defined $file && length $file;
    my $abs = abs_path($file);
    return (undef, undef, 'source_file_unreadable') unless defined $abs && -f $abs;
    my $prefix = $PROCESSOR_LIB_ABS . '/';
    return (undef, undef, 'source_outside_selected_lib') unless index($abs, $prefix) == 0;
    open(my $fh, '<:raw', $abs) or return (undef, undef, 'source_file_unreadable');
    local $/;
    my $bytes = <$fh>;
    close($fh) or return (undef, undef, 'source_file_unreadable');
    return (File::Spec->abs2rel($abs, $PROCESSOR_LIB_ABS), sha256_hex($bytes), undef);
}

sub processor_deparse {
    my ($cv) = @_;
    return undef unless eval { require B::Deparse; 1 };
    my $text = eval { B::Deparse->new('-p', '-sC')->coderef2text($cv) };
    # Preserve the exact B::Deparse text.  The generated descriptor hashes
    # this input, so whitespace normalization here would make independently
    # equivalent live facts look stale for no source-semantic reason.
    return defined $text ? txt($text) : undef;
}

sub processor_code_source_fact;
sub processor_code_ref_fact {
    my ($cv, $fallback_name, $ancestors, $depth) = @_;
    $ancestors //= {};
    $depth //= 0;
    my $display_name = defined $fallback_name ? $fallback_name : '';
    return processor_unresolved_fact($display_name, 'dependency_depth_exceeded') if $depth > 8;
    return processor_unresolved_fact($display_name, 'code_ref_unavailable') unless $cv;
    my $name = sub_name($cv);
    return processor_unresolved_fact($display_name, 'code_name_unavailable') unless length $name;
    return processor_unresolved_fact($name, 'dependency_cycle') if $ancestors->{$name};
    my $body = processor_deparse($cv);
    return processor_unresolved_fact($name, 'deparse_unavailable') unless defined $body;
    my ($source_file, $source_sha256, $source_error) = processor_source_file_fact($cv);
    return processor_unresolved_fact($name, $source_error) if defined $source_error;
    my %fact = (
        __perl => 'CODE', __opaque => JSON::PP::true, __name => $name,
        resolved => JSON::PP::true, __deparse => $body,
        source_file => $source_file, source_sha256 => $source_sha256,
    );
    # Keep the processor record bounded.  The generic unsigned-reader
    # contract already authenticates Get16u's transitive mechanism; repeating
    # ProcessBinaryData's full helper closure per table would make the oracle
    # stream unboundedly large.  A bare Get16u dispatch is the one direct
    # operand the staged word directory must bind.
    my %next_ancestors = (%$ancestors, $name => 1);
    my %dependencies;
    my $package = $name;
    $package =~ s/::[A-Za-z_]\w*$//;
    if ($body =~ /(?<![\w:>])Get16u\s*\(/) {
        my $binding = "${package}::Get16u";
        $dependencies{$binding} = processor_code_source_fact($binding, \%next_ancestors, $depth + 1);
    }
    $fact{dependencies} = \%dependencies if %dependencies;
    return \%fact;
}

sub processor_code_source_fact {
    my ($name, $ancestors, $depth) = @_;
    $ancestors //= {};
    $depth //= 0;
    return processor_unresolved_fact($name, 'dependency_depth_exceeded') if $depth > 8;
    return processor_unresolved_fact($name, 'dependency_cycle') if $ancestors->{$name};
    return processor_unresolved_fact($name, 'invalid_fully_qualified_name')
        unless $name =~ /^(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*$/;
    no strict 'refs';
    my $cv = *{$name}{CODE};
    return processor_unresolved_fact($name, 'code_ref_unavailable') unless $cv;
    return processor_code_ref_fact($cv, $name, $ancestors, $depth);
}

# Whether `$t` is in the IFD scope: ProcessExif is ExifTool's default
# PROCESS_PROC (ExifTool.pm's ProcessDirectory falls back to it), so a tag
# table with no PROCESS_PROC at all is one, and so is one naming it
# explicitly. "Tag table" is decided by ExifTool's own `%specialTags` (the
# table-level keys ExifTool.pm:1230-1237 recognises), NOT by dump_tables.pl's
# hand list: a hash qualifies when it carries such a key, or when some
# ordinary (non-special, non-underscore) key holds a structured entry. A
# plain lookup hash of scalars (a lens-type map with no metadata) does not.
sub in_ifd_scope {
    my ($t, $pp) = @_;
    if (defined $pp) {
        return 0 unless ref $pp eq 'CODE';
        return 0 unless sub_name($pp) =~ /::Exif::ProcessExif$/;
    }
    no warnings 'once';
    my $has_meta = grep { $Image::ExifTool::specialTags{$_} } keys %$t;
    my $struct = grep { !$Image::ExifTool::specialTags{$_} && !/^_/ && ref $t->{$_} } keys %$t;
    return ($has_meta || $struct) ? 1 : 0;
}

# ExifTool.pm:5878-5894 `ExpandFlags`, re-derived: the keys a tag's `Flags`
# would materialise once `SetupTagTable` runs. Returned as a hash so a
# reader can ask for one flag; the result overrides a direct key exactly
# as `$$tagInfo{$_} = 1` would.
sub expanded_flags {
    my ($e) = @_;
    my %out;
    my $flags = $e->{Flags};
    return \%out unless defined $flags;
    if (ref $flags eq 'ARRAY') {
        $out{$_} = 1 for @$flags;
    } elsif (ref $flags eq 'HASH') {
        $out{$_} = $flags->{$_} for keys %$flags;
    } elsif (!ref $flags) {
        $out{$flags} = 1;
    }
    return \%out;
}

sub flag_fact {
    my ($e, $x, $name) = @_;
    return exists $x->{$name} ? $x->{$name} : $e->{$name};
}

# The custom-processor inventory is intentionally source-shaped.  It never
# recognizes a processor body or a generated layout: consumers join a table's
# independently captured PROCESS_PROC fact to these raw table/entry records.
# A property distinguishes absent from present-but-undef and retains reference
# kinds without evaluating arbitrary native code.
sub processor_native_value {
    my ($value, $depth, $seen) = @_;
    $depth //= 0;
    $seen //= {};
    return { kind => 'undef' } unless defined $value;
    return { kind => 'deep' } if $depth > 4;
    my $kind = ref($value);
    return { kind => 'scalar', value => txt($value) } unless $kind;
    if ($kind eq 'CODE') {
        my $name = sub_name($value);
        return { kind => 'code', (length($name) ? (name => $name) : ()) };
    }
    if ($kind eq 'SCALAR') {
        return { kind => 'scalar_ref', value => processor_native_value($$value, $depth + 1, $seen) };
    }
    my $id = refaddr($value);
    return { kind => 'cycle', ref_kind => $kind } if defined $id && $seen->{$id};
    $seen->{$id} = 1 if defined $id;
    my $result;
    if ($kind eq 'ARRAY') {
        $result = { kind => 'array', items => [ map { processor_native_value($_, $depth + 1, $seen) } @$value ] };
    } elsif ($kind eq 'HASH') {
        my %map;
        for my $key (sort keys %$value) {
            $map{txt($key)} = processor_native_value($value->{$key}, $depth + 1, $seen);
        }
        $result = { kind => 'hash', map => \%map };
    } else {
        $result = { kind => 'ref', ref_kind => $kind };
    }
    delete $seen->{$id} if defined $id;
    return $result;
}

sub processor_native_property {
    my ($hash, $key) = @_;
    return { present => JSON::PP::false } unless ref($hash) eq 'HASH' && exists $hash->{$key};
    return { present => JSON::PP::true, value => processor_native_value($hash->{$key}) };
}

# A SubDirectory may hold the actual target table hash, which is cyclic and not
# a literal edge operand.  Preserve literal edge members exactly while making a
# target reference explicit rather than recursively serializing a second table.
sub processor_native_edge {
    my ($value) = @_;
    return processor_native_value($value) unless ref($value) eq 'HASH';
    my %map;
    for my $key (sort keys %$value) {
        my $item = $value->{$key};
        if ($key eq 'TagTable' && ref($item)) {
            $map{txt($key)} = { kind => 'ref', ref_kind => ref($item) };
        } else {
            $map{txt($key)} = processor_native_value($item);
        }
    }
    return { kind => 'hash', map => \%map };
}

my @PROCESSOR_ROW_PROPERTIES = qw(
    Name Description Format Writable Count Groups Notes Mask BitShift Condition
    PrintConv ValueConv RawConv PrintConvInv ValueConvInv Hook SubDirectory Flags
    Unknown Hidden Avoid Binary Protected List Priority ByteOrder DataMember
    RelatedTag SeparateTable PrintHex Base Offset ChangeBase Require Desire Inhibit
    BitsPerWord BitsTotal FixFormat SubIFD
);

sub processor_entry_name {
    my ($entry) = @_;
    return txt($entry) unless ref($entry);
    return undef unless ref($entry) eq 'HASH';
    my $expanded = expanded_flags($entry);
    my $name = exists($expanded->{Name}) ? $expanded->{Name} : $entry->{Name};
    return defined($name) && !ref($name) ? txt($name) : undef;
}

sub processor_entry_fact {
    my ($entry) = @_;
    my $kind = ref($entry) || 'SCALAR';
    my %fact = (entry_kind => $kind, name => processor_entry_name($entry));
    return \%fact unless ref($entry) eq 'HASH';
    my %properties;
    for my $key (@PROCESSOR_ROW_PROPERTIES) {
        $properties{$key} = $key eq 'SubDirectory'
            ? (exists($entry->{$key})
                ? { present => JSON::PP::true, value => processor_native_edge($entry->{$key}) }
                : { present => JSON::PP::false })
            : processor_native_property($entry, $key);
    }
    my $expanded = expanded_flags($entry);
    my %expanded_properties;
    for my $key (sort keys %$expanded) {
        $expanded_properties{txt($key)} = processor_native_value($expanded->{$key});
    }
    $fact{properties} = \%properties;
    $fact{expanded_properties} = \%expanded_properties;
    my %effective_flags;
    for my $key (qw(Unknown Binary List Protected Avoid Priority)) {
        $effective_flags{$key} = processor_native_value(flag_fact($entry, $expanded, $key));
    }
    $fact{effective_flags} = \%effective_flags;
    return \%fact;
}

# Preserve every known ExifTool table-level special tag with explicit
# presence. This is intentionally driven by ExifTool's own `%specialTags`, not
# a word/Canon allowlist: a newly introduced read-affecting table property is
# visible to a verifier even when absent on today's selected tables.
sub processor_table_metadata {
    my ($table) = @_;
    my %metadata;
    for my $key (sort keys %Image::ExifTool::specialTags) {
        $metadata{txt($key)} = processor_native_property($table, $key);
    }
    return \%metadata;
}

sub processor_row_records {
    my ($table) = @_;
    my @records;
    for my $key (sort keys %$table) {
        next if $key =~ /^_/ || $Image::ExifTool::specialTags{$key};
        my $entry = $table->{$key};
        if (ref($entry) eq 'ARRAY') {
            for my $variant (0 .. $#$entry) {
                push @records, [ clean($key), "$variant", processor_entry_fact($entry->[$variant]) ];
            }
        } else {
            push @records, [ clean($key), '-', processor_entry_fact($entry) ];
        }
    }
    return @records;
}

# '-' when absent, `__REF__` for a reference, else the cleaned text.
sub dash_text {
    my ($v) = @_;
    return '-' unless defined $v;
    return ref $v ? '__REF__' : clean($v);
}

# Resolve effective SubDirectory processors only after all modules have loaded:
# an override or target table can be rebound by a later module.  The raw
# `SUBDIR` record remains the source spelling; this separate fact is the
# authenticated executable binding the generated serial edge depends on.
my @ifd_subdir_processors;

# Emit every IFD row for one tag-info entry `$e` at `$key`. `$plain` is true
# for a scalar-keyed entry and false for a `_variants` alternative (only the
# former gets a CONDITION row -- see the header).
sub emit_ifd_entry {
    my ($mod, $sym, $key, $e, $plain) = @_;
    my $name = ref $e eq 'HASH' ? $e->{Name} : $e;
    return unless defined $name && !ref $name;
    my @p = ('IFD', $mod, $sym, $key);
    print join("\t", @p, 'NAME', clean($name)), "\n";
    return unless ref $e eq 'HASH';
    my $x = expanded_flags($e);

    my $fmt = $e->{Format};
    print join("\t", @p, 'FORMAT', clean($fmt)), "\n" if defined $fmt && !ref $fmt;
    my $cnt = $e->{Count};
    print join("\t", @p, 'COUNT', clean($cnt)), "\n" if defined $cnt && !ref $cnt;
    # `Writable => 1`/`0` is a yes/no about writability, not a format; only
    # a spelling says which value domain the generator may assume.
    my $w = $e->{Writable};
    print join("\t", @p, 'WRITABLE', clean($w)), "\n"
        if defined $w && !ref $w && $w !~ /^\d+$/;

    my $tgrp = $e->{Groups};
    if (ref $tgrp eq 'HASH') {
        print join("\t", @p, 'GROUPS',
            map { defined $tgrp->{$_} && !ref $tgrp->{$_} ? clean($tgrp->{$_}) : '' } (0, 1, 2)
        ), "\n";
    }

    my $prio = flag_fact($e, $x, 'Priority');
    print join("\t", @p, 'FLAGS',
        (map { flag_fact($e, $x, $_) ? 1 : 0 } qw(Unknown Binary List Protected Avoid)),
        (defined $prio && !ref $prio ? clean($prio) : '-'),
    ), "\n";

    my $rc = $e->{RawConv};
    if (defined $rc) {
        my $s;
        if (ref $rc eq 'CODE') { $s = '__CODE__' }
        elsif (ref $rc)        { $s = '__REF__' }
        else {
            $s = clean($rc);
            $s =~ s/\s+/ /g;
            $s =~ s/^ //;
            $s =~ s/ $//;
        }
        print join("\t", @p, 'RAWCONV', $s), "\n";
    }
    print join("\t", @p, 'HOOK', 1), "\n" if defined $e->{Hook};
    print join("\t", @p, 'CONDITION', 1), "\n" if $plain && defined $e->{Condition};

    if (defined $e->{SubDirectory}) {
        my $sd = $e->{SubDirectory};
        my ($tagtable, $start, $base, $proc, $bo, $validate, $max, $dir) = ('-') x 8;
        $validate = 0;
        if (ref $sd eq 'HASH') {
            $tagtable = dash_text($sd->{TagTable});
            $start = dash_text($sd->{Start});
            $base = dash_text($sd->{Base});
            if (defined $sd->{ProcessProc}) {
                $proc = ref $sd->{ProcessProc} eq 'CODE'
                    ? (sub_name($sd->{ProcessProc}) || '__CODE__')
                    : dash_text($sd->{ProcessProc});
            }
            $bo = dash_text($sd->{ByteOrder});
            $validate = defined $sd->{Validate} ? 1 : 0;
            $max = dash_text($sd->{MaxSubdirs});
            $dir = dash_text($sd->{DirName});
        }
        my $fix = $e->{FixFormat};
        my $subifd = (flag_fact($e, $x, 'SubIFD')
            || (defined $fix && !ref $fix && $fix eq 'ifd')) ? 1 : 0;
        print join("\t", @p, 'SUBDIR', $tagtable, $start, $base, $proc, $bo, $validate,
                   dash_text($fix), $subifd, $max, $dir), "\n";
        push @ifd_subdir_processors, {
            module => $mod, table => $sym, key => $key, tagtable => $tagtable,
            override => (ref $sd eq 'HASH' ? $sd->{ProcessProc} : undef),
        };
        if (ref $sd eq 'HASH' && defined $sd->{Validate} && !ref $sd->{Validate}) {
            print join("\t", @p, 'VALIDATION', clean($sd->{Validate}),
                       keyed_validation_source($sd->{Validate}),
                       JSON::PP->new->canonical->encode(keyed_validation_helper_fact($sd->{Validate}))), "\n";
        }
    }

    my $pc = $e->{PrintConv};
    if (defined $pc && ref $pc && ref $pc ne 'HASH') {
        print join("\t", @p, 'PCREF', ref $pc), "\n";
    }
    # A scalar PrintConv is Perl source (an expression) the generator either
    # compiles or refuses; the row lets verify.py tell a legitimate refusal
    # (`Omitted { print_conv: true }` where ExifTool DOES convert) from a
    # refusal of nothing.
    if (defined $pc && !ref $pc) {
        print join("\t", @p, 'PCEXPR', 1), "\n";
    }
    return unless ref $pc eq 'HASH';
    if (ref $pc->{BITMASK} eq 'HASH') {
        for my $bk (sort keys %{$pc->{BITMASK}}) {
            print join("\t", @p, 'BITMASK', clean($bk), clean($pc->{BITMASK}{$bk})), "\n";
        }
    }
    if (exists $pc->{OTHER}) {
        my $print_hex = flag_fact($e, $x, 'PrintHex') ? '1' : '';
        print join("\t", @p, 'OTHER', $print_hex), "\n";
    }
    for my $ck (sort keys %$pc) {
        next if $ck =~ /^(BITMASK|OTHER|Notes|PrintHex|SeparateTable)$/;
        next if ref $pc->{$ck};
        print join("\t", @p, 'ENUM', clean($ck), clean($pc->{$ck})), "\n";
    }
}

sub emit_ifd_table {
    my ($mod, $sym, $t) = @_;
    my $g = $t->{GROUPS};
    my @graw = ref $g eq 'HASH'
        ? map { defined $g->{$_} && !ref $g->{$_} ? clean($g->{$_}) : '' } (0, 1, 2)
        : ('', '', '');
    print join("\t", 'IFD', $mod, $sym, '', 'TGROUPS', @graw), "\n";
    print join("\t", 'IFD', $mod, $sym, '', 'SETGROUP1', clean($t->{SET_GROUP1})), "\n"
        if defined $t->{SET_GROUP1} && !ref $t->{SET_GROUP1};
    print join("\t", 'IFD', $mod, $sym, '', 'PRIORITY', clean($t->{PRIORITY})), "\n"
        if defined $t->{PRIORITY} && !ref $t->{PRIORITY};
    for my $k (sort keys %$t) {
        next if $k !~ /^-?[\d.]+$/;
        my $e = $t->{$k};
        if (ref $e eq 'ARRAY') {
            my $i = 0;
            for my $alt (@$e) {
                emit_ifd_entry($mod, $sym, "$k#$i", $alt, 0);
                $i++;
            }
            next;
        }
        emit_ifd_entry($mod, $sym, $k, $e, 1);
    }
}

opendir(my $dh, "$LIB/Image/ExifTool") or die "opendir: $!";
my @mods = sort map { s/\.pm$//r } grep { /\.pm$/ } readdir($dh);
closedir $dh;
my %skip = map { $_ => 1 } qw(BuildTagLookup TagLookup TagNames Writer Shift Import Validate Geolocation);
my %processor_tables;

for my $mod (grep { !$skip{$_} } @mods) {
    die "a module named IFD would collide with the IFD row kind column\n" if $mod eq 'IFD';
    my $pkg = "Image::ExifTool::$mod";
    eval "require $pkg; 1" or next;
    no strict 'refs';
    for my $sym (sort keys %{"${pkg}::"}) {
        next if $sym =~ /::$/;
        my $t = eval { \%{"${pkg}::${sym}"} };
        next unless $t && ref $t eq 'HASH';
        # Binary tables only -- matching the generator's scope.  A table
        # qualifies via an explicit scalar FORMAT, or by being processed with
        # ProcessBinaryData (where FORMAT defaults to int8u).  Derived here
        # independently of dump_tables.pl on purpose.
        my $has_format = defined $t->{FORMAT} && !ref $t->{FORMAT};
        my $pp = $t->{PROCESS_PROC};
        # Record every table-owned PROCESS_PROC, including custom processors
        # outside the current binary/IFD/keyed output scopes.  Facts are
        # emitted only after all modules load, when package dispatch is final.
        $processor_tables{"$mod\t$sym"} = { cv => $pp, table => $t } if ref $pp eq 'CODE';
        my $is_bin = 0;
        if (ref $pp eq 'CODE') {
            my $cv = eval { B::svref_2object($pp) };
            if ($cv && $cv->isa('B::CV')) {
                my $gv = eval { $cv->GV };
                if ($gv && ref($gv) ne 'B::SPECIAL') {
                    my $n = eval { $gv->STASH->NAME . '::' . $gv->NAME } // '';
                    $is_bin = 1 if $n =~ /ProcessBinaryData$/;
                }
            }
        }
        # Slice I-1: the IFD scope is decided independently of the binary
        # one, and a table may sit in both (a scalar FORMAT with no
        # PROCESS_PROC); its two row sets are told apart by the `IFD` column.
        my $is_ifd = in_ifd_scope($t, $pp);
        my $is_keyed = ref $pp eq 'CODE'
            && sub_name($pp) eq 'Image::ExifTool::CanonRaw::ProcessCanonRaw';
        next unless $has_format || $is_bin || $is_ifd || $is_keyed;

        if ($is_keyed) {
            my $g = $t->{GROUPS};
            my @graw = ref $g eq 'HASH'
                ? map { defined $g->{$_} && !ref $g->{$_} ? clean($g->{$_}) : '' } (0, 1, 2)
                : ('', '', '');
            print join("\t", 'KEYED', $mod, $sym, '', 'TGROUPS', @graw), "\n";
            for my $k (sort keys %$t) {
                next if $k !~ /^\d+$/;
                my $e = $t->{$k};
                if (ref $e eq 'ARRAY') {
                    my $i = 0;
                    for my $alt (@$e) {
                        emit_keyed_entry($mod, $sym, "$k#$i", $alt, $t);
                        $i++;
                    }
                } else {
                    emit_keyed_entry($mod, $sym, $k, $e, $t);
                }
            }
        }

        if ($has_format || $is_bin) {
        # Step 26: the table's RAW GROUPS, exactly as the module declares it
        # (this reads the hash BEFORE GetTagTable would default it, so
        # verify.py can re-derive the defaulting rule rather than be handed
        # its result).
        my $g = $t->{GROUPS};
        my @graw = ref $g eq 'HASH'
            ? map { defined $g->{$_} && !ref $g->{$_} ? clean($g->{$_}) : '' } (0, 1, 2)
            : ('', '', '');
        print join("\t", $mod, $sym, '', 'TGROUPS', @graw), "\n";

        for my $k (sort keys %$t) {
            next if $k !~ /^-?[\d.]+$/;
            my $e = $t->{$k};
            if (ref $e eq 'ARRAY') {
                # Step 23: `_variants` -- ExifTool's own arrayref-of-
                # alternatives representation of a model-dependent layout
                # (dump_tables.pl's `_variants`, which codegen.py compiles
                # through a closed `Cond` grammar into `VariantGroup`).
                # Alternatives share one offset, so the plain `$k` key would
                # collide them into a single row; `"$k#$i"` (0-based array
                # position -- the same order dump_tables.pl and codegen.py
                # both walk the Perl array in) disambiguates without
                # changing the plain-field key shape at all: a plain key is
                # never built with `#` in it, so the two key spaces cannot
                # collide with each other either.
                my $i = 0;
                for my $alt (@$e) {
                    emit_entry($mod, $sym, "$k#$i", $alt);
                    $i++;
                }
                next;
            }
            emit_entry($mod, $sym, $k, $e);
        }
        }

        emit_ifd_table($mod, $sym, $t) if $is_ifd;
    }
}

# This stream is independent of dump_tables.pl and is deliberately source-only:
# it gives a verifier current native PROCESS_PROC and package-local reader
# facts without reusing the word-directory recognizer.  It is emitted after
# the module walk so later glob replacements are visible.
my %processor_facts;
for my $key (sort keys %processor_tables) {
    my ($mod, $sym) = split /\t/, $key, 2;
    my $fact = processor_code_ref_fact($processor_tables{$key}{cv},
        "Image::ExifTool::${mod}::${sym}::PROCESS_PROC");
    $processor_facts{$key} = $fact;
    print join("\t", 'NATIVE_PROCESSOR', $mod, $sym,
        JSON::PP->new->canonical->encode($fact)), "\n";
}

# The raw IFD SUBDIR fact says whether ProcessProc was present, but its target
# table and any CODE override are only final after the complete module walk.
# Bind that effective processor here with the same source/deparse provenance
# used for NATIVE_PROCESSOR.  This is source-derived protocol, never a
# Canon/table selector.
for my $edge (sort {
       $a->{module} cmp $b->{module}
    || $a->{table} cmp $b->{table}
    || $a->{key} cmp $b->{key}
} @ifd_subdir_processors) {
    my ($target_mod, $target_table) = ('-', '-');
    if ($edge->{tagtable} =~ /^Image::ExifTool::([A-Za-z_]\w*)::([A-Za-z_]\w*)$/) {
        ($target_mod, $target_table) = ($1, $2);
    }
    my ($origin, $fact);
    if (defined $edge->{override}) {
        $origin = 'override';
        $fact = ref($edge->{override}) eq 'CODE'
            ? processor_code_ref_fact($edge->{override}, 'SubDirectory::ProcessProc')
            : processor_unresolved_fact('SubDirectory::ProcessProc', 'subdirectory_processproc_not_code');
    } elsif ($target_mod ne '-') {
        $origin = 'target';
        $fact = $processor_facts{"$target_mod\t$target_table"}
            // processor_unresolved_fact("Image::ExifTool::${target_mod}::${target_table}::PROCESS_PROC",
                'target_processproc_unavailable');
    } else {
        $origin = 'unresolved';
        $fact = processor_unresolved_fact('SubDirectory::TagTable', 'target_table_unparseable');
    }
    print join("\t", 'IFD', $edge->{module}, $edge->{table}, $edge->{key}, 'PROCESSOR',
        $target_mod, $target_table, $origin, JSON::PP->new->canonical->encode($fact)), "\n";
}
for my $key (sort keys %processor_tables) {
    my ($mod, $sym) = split /\t/, $key, 2;
    my $table = $processor_tables{$key}{table};
    my @rows = processor_row_records($table);
    my $named = scalar grep { defined $_->[2]{name} } @rows;
    my $table_fact = {
        processor => $processor_facts{$key},
        groups => processor_native_property($table, 'GROUPS'),
        format => processor_native_property($table, 'FORMAT'),
        first_entry => processor_native_property($table, 'FIRST_ENTRY'),
        metadata => processor_table_metadata($table),
        row_record_count => scalar(@rows), named_row_count => $named,
    };
    print join("\t", 'NATIVE_PROCESSOR_TABLE', $mod, $sym,
        JSON::PP->new->canonical->encode($table_fact)), "\n";
    for my $row (@rows) {
        print join("\t", 'NATIVE_PROCESSOR_ROW', $mod, $sym, $row->[0], $row->[1],
            JSON::PP->new->canonical->encode($row->[2])), "\n";
    }
}

# Run the primitive oracle in isolation, then authenticate its refs against
# the modules this oracle actually loaded. The snapshot records native reads
# and state transitions; no compiler body recognizer participates here.
my $reader_contract = OxiDex::NativeReaderContract::finalise_loaded_contract(
    OxiDex::NativeReaderContract::capture_isolated_contract(
        $^X, "$FindBin::Bin/dump_binary_reader_contract.pl", $LIB), abs_path($LIB));
print join("\t", 'NATIVE_READER_CONTRACT', 'unsigned16',
    JSON::PP->new->canonical->encode($reader_contract)), "\n";
