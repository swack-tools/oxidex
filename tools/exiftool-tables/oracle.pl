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
#   MODULE  TABLE  INDEX  SUBDIR  TAGTABLE  START  BASE  PROCESSPROC  BYTEORDER  VALIDATE
#                                                    -- field carries a SubDirectory (10)
#
# The trailing empty column on HOOK/VARFMT lines is not decorative: it is
# what keeps them from colliding with a NAME line on column count (both
# would otherwise be 4 columns, and a tag genuinely named "Hook" is not
# impossible). SUBDIR carries the raw facts Step 27's `verify.py` needs to
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

    my $pc = $e->{PrintConv};
    return unless ref $pc eq 'HASH';
    for my $ck (sort keys %$pc) {
        next if $ck =~ /^(BITMASK|OTHER|Notes|PrintHex|SeparateTable)$/;
        next if ref $pc->{$ck};
        print join("\t", $mod, $sym, $key, 'ENUM', clean($ck),
                   clean($pc->{$ck})), "\n";
    }
}

opendir(my $dh, "$LIB/Image/ExifTool") or die "opendir: $!";
my @mods = sort map { s/\.pm$//r } grep { /\.pm$/ } readdir($dh);
closedir $dh;
my %skip = map { $_ => 1 } qw(BuildTagLookup TagLookup TagNames Writer Shift Import Validate Geolocation);

for my $mod (grep { !$skip{$_} } @mods) {
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
        next unless $has_format || $is_bin;

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
}
