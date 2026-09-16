#!/usr/bin/env perl
# Capture the Garmin FIT reader protocol from the selected ExifTool library:
# ProcessFIT's body, the value readers it reaches, its closed-over %baseType
# pad, format sizes, the IsTimeStamp row values (which the table dump keeps
# only as a presence marker) and the interpreter's integer width.
#
# A sidecar in the style of capture_raw_jfif_fact.pl: it never feeds the
# normal table dump. garmin_fit_specs.py refuses the whole protocol unless the
# captured bodies are the ones the executor was reviewed against.
use strict;
use warnings;
use B;
use B::Deparse;
use Config;
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use JSON::PP;
use Sub::Util qw(subname);

my ($lib) = @ARGV;
die "usage: $0 LIB\n" unless defined $lib && @ARGV == 1;
$lib = abs_path($lib) // die "selected library unavailable\n";
unshift @INC, $lib;
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
require Image::ExifTool;
require Image::ExifTool::Garmin;
# Get64u/Get64s are prototype-only in ExifTool.pm; ReadValue autoloads their
# bodies from Writer.pl on first use, so capture the bodies that will run.
require 'Image/ExifTool/Writer.pl';

sub source {
    my ($file) = @_;
    my $path = abs_path($file) // die "native source unavailable\n";
    die "native source outside selected library\n" unless index($path, "$lib/") == 0;
    open my $fh, '<:raw', $path or die "cannot read native source\n";
    local $/;
    my $bytes = <$fh>;
    return (substr($path, length($lib) + 1), sha256_hex($bytes));
}

my $deparser = B::Deparse->new('-p', '-sC');
sub code_fact {
    my ($binding) = @_;
    no strict 'refs';
    my $cv = *{$binding}{CODE};
    return { resolved => JSON::PP::false, __name => $binding, reason => 'code_ref_unavailable' }
        unless ref($cv) eq 'CODE';
    my ($file, $sha256) = source(B::svref_2object($cv)->FILE);
    return {
        resolved => JSON::PP::true, __perl => 'CODE', __name => subname($cv),
        __deparse => $deparser->coderef2text($cv), source_file => $file, source_sha256 => $sha256,
    };
}

# ProcessFIT closes over `my %baseType`; read the live pad value. The invalid
# value is recorded as Perl text: ProcessFIT compares `lc $val eq ...[2]`.
sub base_types {
    my ($cv) = @_;
    my @pad = B::svref_2object($cv)->PADLIST->ARRAY;
    my @names = $pad[0]->ARRAY;
    my @values = $pad[1]->ARRAY;
    my @matches = grep { my $name = eval { $names[$_]->PV }; defined($name) && $name eq '%baseType' } 0 .. $#names;
    return { resolved => JSON::PP::false, reason => 'missing_base_type_pad' } unless @matches == 1;
    my $hash = eval { $values[$matches[0]]->isa('B::HV') ? $values[$matches[0]]->object_2svref : undef };
    return { resolved => JSON::PP::false, reason => 'base_type_pad_unavailable' } unless ref($hash) eq 'HASH';
    my %entries;
    for my $key (sort { $a <=> $b } keys %$hash) {
        my $entry = $hash->{$key};
        return { resolved => JSON::PP::false, reason => 'base_type_entry_shape' }
            unless ref($entry) eq 'ARRAY' && @$entry == 3 && !grep { !defined($_) || ref($_) } @$entry;
        $entries{"$key"} = { format => '' . $entry->[0], fit_name => '' . $entry->[1], invalid => '' . $entry->[2] };
    }
    return { resolved => JSON::PP::true, entries => \%entries };
}

no strict 'refs';
my $process_fit = *{'Image::ExifTool::Garmin::ProcessFIT'}{CODE};
my $base_types = ref($process_fit) eq 'CODE'
    ? base_types($process_fit)
    : { resolved => JSON::PP::false, reason => 'code_ref_unavailable' };
my %format_sizes;
if ($base_types->{resolved}) {
    for my $entry (values %{$base_types->{entries}}) {
        my $size = Image::ExifTool::FormatSize($entry->{format});
        $format_sizes{$entry->{format}} = defined $size ? 0 + $size : undef;
    }
}
my %is_timestamp;
for my $name (sort keys %Image::ExifTool::Garmin::) {
    next unless $name =~ /^[A-Za-z_]\w*$/;
    my $table = \%{"Image::ExifTool::Garmin::$name"};
    next unless %$table && ref($table->{GROUPS}) eq 'HASH';
    for my $key (sort keys %$table) {
        my $row = $table->{$key};
        next unless ref($row) eq 'HASH' && exists $row->{IsTimeStamp};
        $is_timestamp{$name}{"$key"} = '' . ($row->{IsTimeStamp} // '');
    }
}
my %dependencies = map { ("Image::ExifTool::$_" => code_fact("Image::ExifTool::$_")) }
    qw(ReadValue Get64u Get64s GetFloat GetDouble);

print JSON::PP->new->canonical->utf8->pretty->encode({
    kind => 'garmin_fit_reader_protocol_v1',
    native_identity => { perl_version => "$^V", exiftool_version => "$Image::ExifTool::VERSION" },
    process_fit => code_fact('Image::ExifTool::Garmin::ProcessFIT'),
    base_types => $base_types,
    format_sizes => \%format_sizes,
    is_timestamp => \%is_timestamp,
    dependencies => \%dependencies,
    perl_integer => { ivsize => 0 + $Config{ivsize}, uvsize => 0 + $Config{uvsize}, nvsize => 0 + $Config{nvsize} },
});
