#!/usr/bin/env perl
#
# Run dump_tables.pl as shard I of N:
#
#     dump_tables_shard.pl --shard I/N [dump_tables.pl arguments...]
#
# merge_dump_shards.py merges the N shard streams into the exact bytes one
# `dump_tables.pl [arguments...]` run prints.  dump_tables.pl itself is not
# modified -- its bytes are provenance (the hydrated projection records their
# SHA-256, and committed fixtures pin it) -- so this wrapper compiles it
# unchanged and installs its hooks between compilation and execution.
#
# Every shard performs the whole capture in the same order, so module loads,
# runtime links (GetTagInfo), hydrated object numbering and every global fact
# are the same in every shard.  Only two pure computations, which dominate the
# run, are divided round-robin over counters every shard advances identically:
#
#  * Deparsing in the write sidecar.  Each reference-valued write property and
#    each effective WRITE_PROC/CHECK_PROC fact is one unit.  A shard builds a
#    unit it does not own with B::Deparse suppressed (and the deparse cache
#    untouched), then replaces it with an empty placeholder; the owner emits
#    the real one.  Units are this fine because a single row,
#    TagInfoXML::allTables' Exif::Main, is ~960 MB of the 1.25 GB document.
#  * The hydrated Validate-name walk, per tag (DICOM::Main alone is ~107 s).
#    It only feeds a name map; each shard's partial map is unioned by the
#    merge.
#
# Suppressing deparse cannot change what else a shard computes: it only makes
# deparse() return undef inside units, and a unit's content is discarded by
# every shard but its owner.  The merge enforces the rest: every skeleton
# record (all shared structure and global facts) must be byte-identical in
# all N shards.

package OxiDex::DumpShard;
use strict;
use warnings;
use FindBin;
use lib $FindBin::Bin;
use JSON::PP ();
use Scalar::Util qw(refaddr);
use OxiDex::ShardedJson ();

# Defined before any file lexical, so dump_tables.pl can see none of them.
sub run_dump { eval $_[0] or die($@ || "dump_tables.pl failed\n") }

my $spec = (@ARGV >= 2 && $ARGV[0] eq '--shard') ? (shift @ARGV, shift @ARGV)[1] : '';
die "usage: $0 --shard I/N [dump_tables.pl arguments...]\n"
    unless $spec =~ m{^(\d+)/(\d+)$} && $2 >= 1 && $1 < $2;
my ($INDEX, $COUNT) = (0 + $1, 0 + $2);

my $DUMP = "$FindBin::Bin/dump_tables.pl";
my $SOURCE = do {
    open(my $fh, '<:raw', $DUMP) or die "read $DUMP: $!\n";
    local $/;
    <$fh>;
};
die "dump_tables.pl contains __END__/__DATA__; the wrapper cannot compile it\n"
    if $SOURCE =~ /^__(?:END|DATA)__\b/m;

our ($SUPPRESS, $CLAIMING, $KEEP, $IN_HYDRATED_TABLE, $ENCODING, $WROTE) = (0) x 6;
my ($UNIT, $HYDRATED_TAG) = (0, 0);
my (%OWNERS, %UNIONS);

sub owner_of { return $_[0] % $COUNT }

# Build one write-sidecar unit (see the header).
sub unit {
    my ($build) = @_;
    return $build->() unless $CLAIMING;
    my $owner = owner_of($UNIT++);
    my $mine = $owner == $INDEX;
    my $value = do {
        local $SUPPRESS = $SUPPRESS || !$mine;
        local $CLAIMING = 0;
        $build->();
    };
    return $value unless ref $value;
    # warm_find_tag_info_closure reads Exif::Main's rows back after the loop;
    # keep them (their Name properties carry no CODE, so they are exact).
    $value = {} if !$mine && !$KEEP;
    $OWNERS{refaddr($value)} = $owner;
    return $value;
}

sub wrap {
    my ($name, $make) = @_;
    no strict 'refs';
    no warnings 'redefine';
    my $original = \&{"main::$name"};
    die "dump_tables.pl no longer defines $name; update dump_tables_shard.pl\n"
        unless defined &$original;
    *{"main::$name"} = $make->($original);
}

# Runs after dump_tables.pl is compiled (all its subs exist) and before any of
# its top-level statements execute.
sub install {
    wrap(deparse => sub {
        my ($original) = @_;
        sub { return undef if $SUPPRESS; goto &$original };
    });
    # Bypass (never read or fill) the deparse memo while suppressed.
    wrap(code_fact_deparse => sub {
        my ($original) = @_;
        sub { return main::deparse($_[0]) if $SUPPRESS; goto &$original };
    });
    wrap(dump_write_table => sub {
        my ($original) = @_;
        sub {
            local $CLAIMING = 1;
            local $KEEP = $_[2] eq 'Image::ExifTool::Exif::Main';
            return $original->(@_);
        };
    });
    # Only a reference can hold CODE; a scalar property is cheap and exact.
    wrap(write_source_property => sub {
        my ($original) = @_;
        sub {
            my @args = @_;
            my ($hash, $key) = @args;
            return $original->(@args) unless exists $hash->{$key} && ref $hash->{$key};
            return unit(sub { $original->(@args) });
        };
    });
    wrap(effective_write_code_fact => sub {
        my ($original) = @_;
        sub {
            my @args = @_;
            local $CLAIMING = 1;
            return unit(sub { $original->(@args) });
        };
    });
    wrap(dump_hydrated_layout_table => sub {
        my ($original) = @_;
        sub { local $IN_HYDRATED_TABLE = 1; return $original->(@_) };
    });
    # dump_module's read-projection walk (outside a hydrated table) is
    # untouched.  The walk recurses by name, so the original is rebound for
    # the duration of a call: the hook costs one check per tag, not per node.
    wrap(collect_subdirectory_validate_function_names => sub {
        my ($original) = @_;
        sub {
            return if $IN_HYDRATED_TABLE && owner_of($HYDRATED_TAG++) != $INDEX;
            no warnings qw(once redefine);
            local *main::collect_subdirectory_validate_function_names = $original;
            return $original->(@_);
        };
    });
    wrap(dump_hydrated_layout_projection => sub {
        my ($original) = @_;
        sub {
            my $projection = $original->(@_);
            $UNIONS{refaddr($projection->{subdirectory_validate_functions})} = 1;
            return $projection;
        };
    });
    # The final `print $json->encode(\%document)`: stream this shard instead
    # and hand print an empty string.  Every other encode is untouched.
    my $encode = \&JSON::PP::encode;
    no warnings 'redefine';
    *JSON::PP::encode = sub {
        my ($self, $value) = @_;
        if (!$ENCODING && ref($value) eq 'HASH' && exists $value->{modules_ok}
                && exists $value->{native_reader_contracts}) {
            local $ENCODING = 1;
            die "dump_tables.pl encoded its document twice\n" if $WROTE++;
            OxiDex::ShardedJson::write_shard(\*STDOUT, $value, $INDEX, $COUNT,
                owners => \%OWNERS, unions => \%UNIONS);
            return '';
        }
        goto &$encode;
    };
}

# Compile and run dump_tables.pl as package main, reporting its own file name
# and line numbers (its producer digest reads __FILE__, i.e. the unmodified
# script).  dump_tables.pl parses the remaining @ARGV itself.
run_dump("package main; UNITCHECK { OxiDex::DumpShard::install() }\n#line 1 \"$DUMP\"\n$SOURCE\n;1");
die "dump_tables.pl finished without encoding its document\n" unless $WROTE;
close(STDOUT) or die "close stdout: $!\n";
