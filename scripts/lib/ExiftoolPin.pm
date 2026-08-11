package ExiftoolPin;
#
# One resolver, shared by every `scripts/gen_*.pl` table generator, for the
# ExifTool source tree they read from.
#
# Before this module existed, two of these generators hardcoded a fallback to
# a Homebrew-installed ExifTool 13.55 whenever the shared cache directory was
# absent (`gen_leica_lens_types.pl`, `gen_canon_custom_functions2.pl`), and the
# rest read a cache directory without ever checking what release was actually
# sitting there. Both are the same bug AGENTS.md already names for the oracle
# scripts (`scripts/exiftool_oracle.py`): a bare, unverified ExifTool degrades
# silently and produces a confident, wrong answer under a real ExifTool tag
# name, indistinguishable afterwards from a correct one. `.exiftool-version`
# is the repo's only source of truth for which release generated code must be
# transcribed from, so every generator resolves through here, which refuses
# to hand back a path unless the release actually loaded from it matches the
# pin -- never a PATH fallback, never an unpinned "whatever is there".
#
# Usage:
#     use FindBin;
#     use lib "$FindBin::Bin/lib";
#     use ExiftoolPin;
#     my $LIB = ExiftoolPin::resolve();   # dies on any mismatch
#
use strict;
use warnings;
use Cwd qw(abs_path);
use File::Basename qw(dirname);
use File::Spec;

# Walk up from this file to find the repo root (the directory holding
# `.exiftool-version`), so callers work the same regardless of cwd.
sub _repo_root {
    my $dir = dirname(abs_path(__FILE__));   # .../scripts/lib
    for (1 .. 8) {
        return $dir if -f File::Spec->catfile($dir, '.exiftool-version');
        my $parent = dirname($dir);
        last if $parent eq $dir;
        $dir = $parent;
    }
    die "ExiftoolPin: could not find .exiftool-version above " . dirname(abs_path(__FILE__)) . "\n";
}

sub _read_pin {
    my ($root) = @_;
    my $path = File::Spec->catfile($root, '.exiftool-version');
    open(my $fh, '<', $path) or die "ExiftoolPin: cannot read $path: $!\n";
    local $/;
    my $pin = <$fh>;
    close $fh;
    $pin =~ s/^\s+|\s+$//g;
    die "ExiftoolPin: $path is empty\n" if $pin eq '';
    return $pin;
}

# The version ExifTool.pm itself declares, read out of the file directly --
# *before* anything is `require`d, so a mismatch is caught without first
# loading (and thereby trusting) the wrong tree.
sub _lib_version {
    my ($lib) = @_;
    my $pm = File::Spec->catfile($lib, 'Image', 'ExifTool.pm');
    return undef unless -r $pm;
    open(my $fh, '<', $pm) or return undef;
    while (my $line = <$fh>) {
        return $1 if $line =~ /^\s*\$VERSION\s*=\s*['"]([^'"]+)['"]/;
    }
    return undef;
}

# Resolve the ExifTool `lib/` directory to use, verified against the pin.
#
# Resolution order (no step falls through to a different release than the
# pin -- each candidate is checked, and a mismatch is fatal, not skipped):
#
#   1. $OXIDEX_EXIFTOOL_LIB       -- explicit override (CI: the tree it just
#                                     fetched; local: a developer's own path).
#   2. $EXIFTOOL_CACHE_DIR/exiftool/lib, defaulting to
#      /tmp/oxidex-exiftool-cache/exiftool/lib -- the shared pinned oracle
#      tree every other harness in this repo already reads
#      (scripts/exiftool_oracle.py, tools/exiftool-tables/regen.sh's sibling
#      scripts, exiftool-pinned.sh).
#
# There is no third step. A candidate that does not exist, or whose
# Image/ExifTool.pm declares a different $VERSION than .exiftool-version,
# is refused with a message naming exactly what disagreed -- never silently
# swapped for a Homebrew install or whatever else happens to be on PATH.
sub resolve {
    my $root = _repo_root();
    my $pin  = _read_pin($root);

    my @candidates;
    if (defined $ENV{OXIDEX_EXIFTOOL_LIB} && length $ENV{OXIDEX_EXIFTOOL_LIB}) {
        push @candidates, ['$OXIDEX_EXIFTOOL_LIB', $ENV{OXIDEX_EXIFTOOL_LIB}];
    } else {
        my $cache = $ENV{EXIFTOOL_CACHE_DIR} || '/tmp/oxidex-exiftool-cache';
        push @candidates, ["$cache/exiftool/lib", "$cache/exiftool/lib"];
    }

    for my $c (@candidates) {
        my ($label, $lib) = @$c;
        unless (-d $lib) {
            die "ExiftoolPin: $label ($lib) does not exist.\n"
              . "Refusing to fall back to a different ExifTool -- populate it (e.g. via\n"
              . "'just compare-exiftool-full', or clone tag $pin into it) and re-run.\n";
        }
        my $found = _lib_version($lib);
        unless (defined $found) {
            die "ExiftoolPin: $label ($lib) has no readable Image/ExifTool.pm \$VERSION.\n"
              . "Refusing to guess; this does not look like an ExifTool source tree.\n";
        }
        if ($found ne $pin) {
            die "ExiftoolPin: $label ($lib) is ExifTool $found but $root/.exiftool-version pins $pin.\n"
              . "Refusing to transcribe against an unpinned release -- see AGENTS.md\n"
              . "('Never grade against an unpinned ExifTool'). Point \$OXIDEX_EXIFTOOL_LIB at\n"
              . "a $pin checkout, or update .exiftool-version and re-run everywhere.\n";
        }
        return $lib;
    }
    die "ExiftoolPin: unreachable\n";
}

1;
