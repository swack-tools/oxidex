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
#   MODULE  TABLE  INDEX  SUBDIR  (empty)           -- field carries a SubDirectory (5)
#   MODULE  TABLE  INDEX  VARFMT  (empty)           -- field's Format is var_* (5)
#
# The trailing empty column on HOOK/SUBDIR/VARFMT lines is not decorative: it
# is what keeps them from colliding with a NAME line on column count (both
# would otherwise be 4 columns, and a tag genuinely named "Hook" is not
# impossible).

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
            next if ref $e eq 'ARRAY';          # variants: generator skips these
            my $name = ref $e eq 'HASH' ? $e->{Name} : $e;
            next unless defined $name && !ref $name;
            print join("\t", $mod, $sym, $k, clean($name)), "\n";

            next unless ref $e eq 'HASH';

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
                print join("\t", $mod, $sym, $k, 'MASK', $mask, $shift), "\n";
            }

            # Hook and SubDirectory are the two constructs codegen.py records
            # but cannot execute (see tools/exiftool-tables/codegen.py's
            # `omitted_for`). Presence, not content, is what a caller needs to
            # know -- a Hook can rewrite later fields' format/byte order
            # in ways this generator does not run, and a SubDirectory means the
            # bytes are the entry to a nested table, not this field's value.
            print join("\t", $mod, $sym, $k, 'HOOK', ''), "\n" if defined $e->{Hook};
            print join("\t", $mod, $sym, $k, 'SUBDIR', ''), "\n" if defined $e->{SubDirectory};

            # A `var_*` Format is data-dependent width: ExifTool computes the
            # real byte offset by walking the bytes, so the generator's static
            # `index * increment` formula is unsound for every field at or
            # past this one (`offsets_sound_until`).
            my $fmt = $e->{Format};
            print join("\t", $mod, $sym, $k, 'VARFMT', ''), "\n"
                if defined $fmt && !ref $fmt && $fmt =~ /^var_/;

            my $pc = $e->{PrintConv};
            next unless ref $pc eq 'HASH';
            for my $ck (sort keys %$pc) {
                next if $ck =~ /^(BITMASK|OTHER|Notes|PrintHex|SeparateTable)$/;
                next if ref $pc->{$ck};
                print join("\t", $mod, $sym, $k, 'ENUM', clean($ck),
                           clean($pc->{$ck})), "\n";
            }
        }
    }
}
