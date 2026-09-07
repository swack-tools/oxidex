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
#   MODULE  TABLE  ''     TGROUPS G0  G1  G2        -- table's raw GROUPS (7)
#   MODULE  TABLE  INDEX  GROUPS  G0  G1  G2        -- tag's own Groups (7)
#   MODULE  TABLE  INDEX  SUBDIR  TAGTABLE  START  BASE  PROCESSPROC  BYTEORDER  VALIDATE
#                                                    -- field carries a SubDirectory (10)
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

my $LIB = shift @ARGV or die "usage: $0 <exiftool-lib-dir>\n";
unshift @INC, $LIB;
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

# '-' when absent, `__REF__` for a reference, else the cleaned text.
sub dash_text {
    my ($v) = @_;
    return '-' unless defined $v;
    return ref $v ? '__REF__' : clean($v);
}

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
    }

    my $pc = $e->{PrintConv};
    if (defined $pc && ref $pc && ref $pc ne 'HASH') {
        print join("\t", @p, 'PCREF', ref $pc), "\n";
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
        next unless $has_format || $is_bin || $is_ifd;

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
