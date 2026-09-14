#!/usr/bin/env perl
#
# Record the complete, hydrated ExifTool table universe.  This is a catalog
# sidecar, not a layout dump: dump_tables.pl remains the source for row-level
# layout and conversion facts.
use strict;
use warnings;
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use File::Spec ();
use File::Basename qw(basename);
use FindBin;
use JSON::PP ();

my $repo_root = File::Spec->catdir($FindBin::Bin, '..', '..');
if (@ARGV >= 2 && $ARGV[0] eq '--repo-root') {
    shift @ARGV;
    $repo_root = shift @ARGV;
}
my $lib = shift @ARGV;
die "usage: dump_hydrated_catalog.pl [--repo-root <repo>] <pinned-exiftool-lib>\n"
    unless defined($lib) && !@ARGV;
$repo_root = abs_path($repo_root);
die "repository root is unavailable\n" unless defined($repo_root) && -d $repo_root;
my $pin_file = File::Spec->catfile($repo_root, '.exiftool-version');
open(my $pin_fh, '<:raw', $pin_file) or die "cannot read repository ExifTool pin: $!\n";
my $pin = <$pin_fh>;
close($pin_fh) or die "cannot close repository ExifTool pin: $!\n";
$pin =~ s/\s+\z//;
die "repository ExifTool pin is malformed\n" unless $pin =~ /\A\d+\.\d+\z/;
$lib = abs_path($lib);
die "pinned ExifTool lib is unavailable\n" unless defined($lib) && -d $lib;
unshift @INC, $lib;

# Build the pinned default catalog, independent of a developer's user config.
# ExifTool consults this global while the module is first required.
{ no warnings 'once'; $Image::ExifTool::configFile = ''; }
require Image::ExifTool;
require Image::ExifTool::BuildTagLookup;
no warnings 'once'; # These package globals are the documented hydrated registry.
die "selected ExifTool version $Image::ExifTool::VERSION does not match repository pin $pin\n"
    unless "$Image::ExifTool::VERSION" eq $pin;

sub source_fact {
    my ($inc_key) = @_;
    my $loaded = $INC{$inc_key};
    die "required catalog source $inc_key was not loaded\n" unless defined $loaded;
    my $absolute = abs_path($loaded);
    my $prefix = "$lib/";
    die "catalog source $inc_key was not loaded from the selected library\n"
        unless defined($absolute) && -f $absolute && index($absolute, $prefix) == 0;
    open(my $fh, '<:raw', $absolute) or die "cannot read $absolute: $!\n";
    local $/;
    my $bytes = <$fh>;
    close($fh) or die "cannot close $absolute: $!\n";
    return {
        library_relative_path => File::Spec->abs2rel($absolute, $lib),
        sha256 => sha256_hex($bytes),
    };
}

# BuildTagLookup calls LoadAllTables(), then adds Shortcuts as its distinct
# pseudo-table.  Calling the loader first makes that ordering explicit and lets
# this producer refuse a future change that stops hydrating the table registry.
Image::ExifTool::LoadAllTables();
my $builder = Image::ExifTool::BuildTagLookup->new;

my @tables;
my %kind_counts;
for my $full_name (sort keys %Image::ExifTool::allTables) {
    my $kind = $full_name eq 'Image::ExifTool::Extra' ? 'extra_generated'
        : $full_name eq 'Image::ExifTool::Composite' ? 'composite_aggregate'
        : 'hydrated_table';
    push @tables, { full_name => $full_name, kind => $kind };
    ++$kind_counts{$kind};
}
die "hydrated table registry is empty\n" unless @tables;

# Preserve every non-hidden, non-subdirectory variant counted by the native
# BuildTagLookup traversal. Name presence is catalog evidence only. The source
# row key and variant index remain separate from its public spelling.
my (@entries, @containers, %public_names);
my $et = Image::ExifTool->new;
for my $full_name (sort keys %Image::ExifTool::allTables) {
    my $table = Image::ExifTool::GetTagTable($full_name);
    for my $raw_key (sort { "$a" cmp "$b" } Image::ExifTool::TagTableKeys($table)) {
        my @variants = Image::ExifTool::GetTagInfoList($table, $raw_key);
        # Native BuildTagLookup hides the whole ID when its first variant is
        # Hidden, otherwise it hides only individual flagged variants.
        next if @variants && $variants[0]{Hidden};
        for my $index (0 .. $#variants) {
            my $row = $variants[$index];
            next if $row->{Hidden};
            my $name = $row->{Name};
            die "catalog row has no public name\n" unless defined($name) && length($name);
            my @groups = $et->GetGroup($row);
            my $entry = {
                table => $full_name,
                raw_key => "$raw_key",
                variant_index => $index,
                name => "$name",
                normalized_name => lc($name),
                groups => { map { ("$_", $groups[$_]) } (0 .. 2) },
                unknown => $row->{Unknown} ? JSON::PP::true : JSON::PP::false,
                no_lookup => ($table->{VARS} || {})->{NO_LOOKUP} ? JSON::PP::true : JSON::PP::false,
            };
            if ($row->{SubDirectory}) {
                # These are catalog navigation/container rows, excluded from
                # BuildTagLookup's total-tags denominator, but never discarded.
                push @containers, $entry;
            } else {
                push @entries, $entry;
                $public_names{lc($name)} = 1;
            }
        }
    }
}
die "catalog entry projection differs from native BuildTagLookup total\n"
    unless @entries == $builder->{COUNT}{'total tags'};

my $shortcut_count = scalar keys %Image::ExifTool::Shortcuts::Main;
die "shortcut catalog was not hydrated\n" unless $shortcut_count;

my %provenance;
for my $key (sort grep { m{\AImage/ExifTool(?:\.pm|/)} } keys %INC) {
    $provenance{$key} = source_fact($key);
}

my $document = {
    schema => 'oxidex_hydrated_catalog_universe_v1',
    exiftool_version => "$Image::ExifTool::VERSION",
    producer => {
        kind => 'pinned_build_tag_lookup_hydrated_catalog_v1',
        expected_exiftool_version => $pin,
        sources => \%provenance,
    },
    capture_environment => {
        perl_version => "$^V",
        perl_executable_basename => basename($^X),
    },
    counts => {
        hydrated_tables => scalar(@tables),
        hydrated_table_kinds => \%kind_counts,
        shortcut_entries => $shortcut_count,
        catalog_unique_tag_names => 0 + $builder->{COUNT}{'unique tag names'},
        catalog_total_tag_entries => 0 + $builder->{COUNT}{'total tags'},
        distinct_case_insensitive_entry_names => scalar(keys %public_names),
        catalog_container_rows_outside_total => scalar(@containers),
    },
    entries => \@entries,
    unique_names => [ sort keys %public_names ],
    container_rows_outside_total => \@containers,
    denominator_definitions => {
        catalog_total_tag_entries => 'Native BuildTagLookup COUNT total tags; includes non-hidden non-SubDirectory variants, excluding shortcuts/plugins.',
        catalog_unique_tag_names => 'Native BuildTagLookup COUNT unique tag names; retained verbatim, not assumed equal to a distinct-name set.',
        distinct_case_insensitive_entry_names => 'Cardinality of Perl lc(Name) over the enumerated catalog entries.',
    },
    families => {
        hydrated_tables => \@tables,
        shortcuts => [{
            full_name => 'Image::ExifTool::Shortcuts::Main',
            kind => 'shortcut_macro_table',
            entry_count => $shortcut_count,
        }],
    },
};
print JSON::PP->new->canonical->pretty->utf8->encode($document);
