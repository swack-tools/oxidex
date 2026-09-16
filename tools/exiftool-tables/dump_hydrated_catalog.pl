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

# Attach the TagNames "Writable" column to every enumerated row.  The column
# is computed inside BuildTagLookup->new and retained only in TAG_NAME_INFO
# rows of [TagID string, [names], [writable], ...], one row per tag ID, with
# consecutive identical (name, write group, writable) variants collapsed.  The
# alignment below recovers each variant's value from that native output and
# refuses any row it cannot account for exactly; it never recomputes the
# column's writability rules.
sub short_table_name {
    # BuildTagLookup::new short-name rules (used by its tag ID formatting).
    my ($short) = @_;
    $short =~ s/^Image::ExifTool:://;
    $short =~ s/::Main$//;
    $short =~ s/::/ /;
    $short =~ s/^(.+)Tags$/\u$1/ unless $short eq 'Nikon AVITags';
    $short =~ s/^Exif\b/EXIF/;
    $short =~ s/^XMP mwg_/XMP mwg-/;
    return $short;
}

sub tag_id_string {
    # BuildTagLookup::new "save TagName information" ID formatting.  Returns
    # undef for the one ID the native builder omits from the documentation.
    my ($table, $short, $tag_id) = @_;
    my $vars = $table->{VARS} || {};
    my $prt_id = $vars->{ID_FMT};
    my $is_iptc = $table->{WRITE_PROC} && $table->{WRITE_PROC} eq \&Image::ExifTool::IPTC::WriteIPTC;
    my $process = $table->{PROCESS_PROC};
    my $binary_data = ($process and ($process eq \&Image::ExifTool::ProcessBinaryData
            or $process eq \&Image::ExifTool::Nikon::ProcessNikonEncrypted
            or $process eq \&Image::ExifTool::Sony::ProcessEnciphered))
        || $vars->{IS_BINARY};
    my $binary_table = $vars->{ID_LABEL} || $binary_data;
    if ($tag_id =~ /^(-)?\d+(\.\d+)?$/) {
        return $tag_id if $1;
        if (defined $prt_id) {
            return sprintf('0x%.4x', $tag_id) if $prt_id eq 'hex';
            return "'${tag_id}'" if $prt_id eq 'str';
            return $tag_id;
        }
        return sprintf('0x%.4x', $tag_id)
            if !$2 && !$binary_table && !$is_iptc && !($short =~ /^CanonCustom/ && $tag_id < 256);
        return $tag_id < 0x10000 ? $tag_id : sprintf('0x%.8x', $tag_id);
    } elsif ($short eq 'DICOM') {
        (my $id = $tag_id) =~ s/_/,/;
        return $id;
    } elsif ($tag_id =~ /^0x([0-9a-f]+)\.(\d+)$/) {
        return $tag_id;
    }
    my $id = $tag_id;
    if ($id =~ s/([\x00-\x1f\x7f-\xff])/'\x'.unpack('H*',$1)/eg) {
        $id =~ s/\\x00/\\0/g;
        return undef if $id eq 'jP\x1a\x1a';
        return qq{"$id"};
    }
    return "'${id}'";
}

sub writable_class {
    # BuildTagLookup POD: anything but "no" is writable; a trailing "*" flag
    # marks a Protected tag that is not writable directly.  A leading "-"
    # links a separate table.  Natively "=struct" replaces the whole column,
    # even a "no" (BuildTagLookup.pm: $writable = "=struct" if $struct), so a
    # struct's class comes from the native writable-tag lookup instead.
    my ($column, $in_write_lookup) = @_;
    (my $value = $column) =~ s/^-//;
    if ($value =~ /^=struct/) {
        die "struct Writable column has no native write-lookup fact\n" unless defined $in_write_lookup;
        return $in_write_lookup ? 'writable' : 'not_writable';
    }
    return 'not_writable' if $value eq '' || $value =~ /^no(?![A-Za-z0-9\[])/;
    my ($flags) = $value =~ /([+\/~!*:_^]*)\z/;
    return 'writable_protected' if index($flags, '*') >= 0;
    return 'writable';
}

my $tag_name_info = $builder->{TAG_NAME_INFO};
die "native TagNames information is unavailable\n" unless ref $tag_name_info eq 'HASH';
# BuildTagLookup's writable-tag lookup (lc name -> table number -> tag ID or
# ID set), numbered in its sorted allTables order.
my $write_lookup = $builder->{TAG_LOOKUP};
die "native write lookup is unavailable\n" unless ref $write_lookup eq 'HASH';
my %table_number;
{ my @ordered = sort keys %Image::ExifTool::allTables; @table_number{@ordered} = 0 .. $#ordered; }
sub in_write_lookup {
    my ($full_name, $table, $raw_key, $name) = @_;
    die "struct row in NO_LOOKUP table $full_name cannot be resolved\n" if ($table->{VARS} || {})->{NO_LOOKUP};
    my $ids = ($write_lookup->{lc $name} || {})->{$table_number{$full_name}};
    return JSON::PP::false unless defined $ids;
    return (ref $ids eq 'HASH' ? exists $ids->{$raw_key} : "$ids" eq $raw_key) ? JSON::PP::true : JSON::PP::false;
}
my %native_writable;
for my $full_name (sort keys %Image::ExifTool::allTables) {
    my $table = Image::ExifTool::GetTagTable($full_name);
    my $short = short_table_name($full_name);
    my $info = $tag_name_info->{$full_name};
    die "native TagNames information is missing for $full_name\n" unless ref $info eq 'ARRAY';
    my %by_id;
    for my $row (@$info) {
        die "duplicate native TagNames ID $row->[0] in $full_name\n" if exists $by_id{$row->[0]};
        $by_id{$row->[0]} = $row;
    }
    my %used;
    for my $raw_key (sort { "$a" cmp "$b" } Image::ExifTool::TagTableKeys($table)) {
        my @variants = Image::ExifTool::GetTagInfoList($table, $raw_key);
        next if @variants && $variants[0]{Hidden};
        my @visible = grep { !$variants[$_]{Hidden} } 0 .. $#variants;
        next unless @visible;
        my $id = tag_id_string($table, $short, "$raw_key");
        unless (defined $id) {
            $native_writable{join("\0", $full_name, "$raw_key", $_)} = {
                state => 'not_listed', column => undef, candidates => [], class => 'not_listed',
                in_write_lookup => undef,
            } for @visible;
            next;
        }
        my $row = $by_id{$id};
        die "no native TagNames row for $full_name [$raw_key] ($id)\n" unless $row;
        die "native TagNames row $id in $full_name is used twice\n" if $used{$id}++;
        my ($names, $columns) = @$row[1, 2];
        die "native TagNames row $id in $full_name is malformed\n"
            unless ref $names eq 'ARRAY' && ref $columns eq 'ARRAY' && @$names == @$columns && @$names;
        # Group consecutive variants and documentation columns by public name.
        my (@variant_runs, @column_runs);
        for my $index (@visible) {
            my $name = $variants[$index]{Name} . ($variants[$index]{Unknown} ? '?' : '');
            if (@variant_runs && $variant_runs[-1][0] eq $name) { push @{$variant_runs[-1][1]}, $index }
            else { push @variant_runs, [$name, [$index]] }
        }
        for my $position (0 .. $#$names) {
            if (@column_runs && $column_runs[-1][0] eq $names->[$position]) { push @{$column_runs[-1][1]}, $columns->[$position] }
            else { push @column_runs, [$names->[$position], [$columns->[$position]]] }
        }
        die "native TagNames names differ from variants for $full_name [$raw_key]\n"
            unless @variant_runs == @column_runs
                && !grep { $variant_runs[$_][0] ne $column_runs[$_][0] } 0 .. $#variant_runs;
        for my $run (0 .. $#variant_runs) {
            my @indexes = @{$variant_runs[$run][1]};
            my @values = @{$column_runs[$run][1]};
            my %distinct = map { ($_ => 1) } @values;
            die "more native TagNames columns than variants for $full_name [$raw_key]\n" if @values > @indexes;
            for my $position (0 .. $#indexes) {
                my ($state, $column, @candidates);
                if (keys(%distinct) == 1) {
                    ($state, $column) = ('determined', $values[0]);
                } elsif (@values == @indexes) {
                    ($state, $column) = ('determined', $values[$position]);
                } else {
                    # Collapsed runs of differing columns cannot be assigned
                    # to variants positionally; keep every candidate.
                    ($state, $column, @candidates) = ('format_ambiguous', undef, sort keys %distinct);
                }
                my @columns = defined $column ? ($column) : @candidates;
                my $lookup = (grep { /^-?=struct/ } @columns)
                    ? in_write_lookup($full_name, $table, "$raw_key", $variants[$indexes[$position]]{Name}) : undef;
                my %classes = map { (writable_class($_, $lookup) => 1) } @columns;
                die "native writability is ambiguous for $full_name [$raw_key]\n" unless keys(%classes) == 1;
                $native_writable{join("\0", $full_name, "$raw_key", $indexes[$position])} = {
                    state => $state, column => $column, candidates => \@candidates, class => (keys %classes)[0],
                    in_write_lookup => $lookup,
                };
            }
        }
    }
    my @unused = grep { !$used{$_} } sort keys %by_id;
    die "native TagNames rows are unaccounted in $full_name: @unused\n" if @unused;
}
my (%writable_counts, %writable_states, %writable_names);
for my $entry (@entries, @containers) {
    my $fact = delete $native_writable{join("\0", @$entry{qw(table raw_key variant_index)})};
    die "catalog row has no native Writable column: $entry->{table} [$entry->{raw_key}]\n" unless $fact;
    $entry->{native_writable} = $fact;
}
die "native Writable facts are unaccounted\n" if %native_writable;
for my $entry (@entries) {
    my $fact = $entry->{native_writable};
    ++$writable_counts{$fact->{class}};
    ++$writable_states{$fact->{state}};
    $writable_names{$entry->{normalized_name}} = 1 if $fact->{class} eq 'writable';
}

my $shortcut_count = scalar keys %Image::ExifTool::Shortcuts::Main;
die "shortcut catalog was not hydrated\n" unless $shortcut_count;

my %provenance;
for my $key (sort grep { m{\AImage/ExifTool(?:\.pm|/)} } keys %INC) {
    $provenance{$key} = source_fact($key);
}

my $document = {
    schema => 'oxidex_hydrated_catalog_universe_v2',
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
        catalog_native_writable_classes => \%writable_counts,
        catalog_native_writable_states => \%writable_states,
        distinct_case_insensitive_writable_names => scalar(keys %writable_names),
    },
    entries => \@entries,
    unique_names => [ sort keys %public_names ],
    container_rows_outside_total => \@containers,
    denominator_definitions => {
        catalog_total_tag_entries => 'Native BuildTagLookup COUNT total tags; includes non-hidden non-SubDirectory variants, excluding shortcuts/plugins.',
        catalog_unique_tag_names => 'Native BuildTagLookup COUNT unique tag names; retained verbatim, not assumed equal to a distinct-name set.',
        distinct_case_insensitive_entry_names => 'Cardinality of Perl lc(Name) over the enumerated catalog entries.',
        catalog_native_writable_classes => 'Entries by the native TagNames Writable column: not_writable ("no"), writable_protected (trailing "*", written only indirectly), writable (anything else), not_listed (omitted from TagNames). An "=struct" column is writable only when the native writable-tag lookup contains the entry.',
        distinct_case_insensitive_writable_names => 'Cardinality of Perl lc(Name) over entries whose native Writable class is writable.',
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
