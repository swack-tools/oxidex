#!/usr/bin/env perl
# Capture WriteExif.pl's final lexical %mandatory map and its closed
# new-directory selection fragment.  This is a sidecar: it never feeds the
# normal reader dump and deliberately has no writer action.
use strict;
use warnings;
use B;
use B::Deparse;
use Digest::SHA qw(sha256_hex);
use File::Spec;
use Cwd qw(abs_path);
use JSON::PP;
use Sub::Util qw(subname);

my ($lib) = @ARGV;
die "usage: $0 LIB\n" unless defined $lib && @ARGV == 1;
$lib = abs_path($lib) // die "library root is unavailable\n";
unshift @INC, $lib;
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
require Image::ExifTool;
require 'Image/ExifTool/Writer.pl';
require 'Image/ExifTool/Exif.pm';
require 'Image/ExifTool/WriteExif.pl';

sub rel_source {
    my ($path) = @_;
    my $absolute = abs_path($path) // die "WriteExif source is unavailable\n";
    my $prefix = File::Spec->catdir($lib) . '/';
    die "WriteExif did not load from selected library\n" unless index($absolute, $prefix) == 0;
    (my $relative = substr($absolute, length($prefix))) =~ s{\\}{/}g;
    return ($absolute, $relative);
}

my ($source_path, $source_file) = rel_source($INC{'Image/ExifTool/WriteExif.pl'});
open my $source_fh, '<:raw', $source_path or die "cannot read WriteExif source\n";
local $/;
my $source = <$source_fh>;
my $source_sha256 = sha256_hex($source);

no strict 'refs';
my $cv = *{'Image::ExifTool::Exif::WriteExif'}{CODE} or die "WriteExif binding is absent\n";
my $actual_name = subname($cv);
die "WriteExif binding was rebound\n" unless $actual_name eq 'Image::ExifTool::Exif::WriteExif';
my $cv_file = abs_path(B::svref_2object($cv)->FILE) // die "WriteExif CV file is unavailable\n";
die "WriteExif CV file differs from selected source\n" unless $cv_file eq $source_path;
my $deparse = B::Deparse->new('-p', '-sC')->coderef2text($cv);
my @contexts = $deparse =~ /((?:\(my|my)\(\$mandatory.*?my\(\$addDirs,\ \@newTags\);)/sg;
die "mandatory executable fragment is absent or ambiguous\n" unless @contexts == 1;
my $context = $contexts[0];
my @pad = B::svref_2object($cv)->PADLIST->ARRAY;
die "WriteExif pad is unavailable\n" unless @pad >= 2;
my @names = $pad[0]->ARRAY;
my @values = $pad[1]->ARRAY;
my @mandatory = grep { defined eval { $names[$_]->PV } && eval { $names[$_]->PV } eq '%mandatory' } 0 .. $#names;
die "mandatory lexical binding is absent or ambiguous\n" unless @mandatory == 1;
my $value = $values[$mandatory[0]];
die "mandatory lexical binding is not a hash\n" unless eval { $value->isa('B::HV') };
my $hash = $value->object_2svref;
die "mandatory lexical binding is tied\n" if tied(%$hash);
sub scalar_or_die {
    my ($value) = @_;
    die "mandatory value is a reference\n" if ref $value;
    die "mandatory value is undefined\n" unless defined $value;
    return $value;
}
my %mandatory_map;
for my $directory (sort keys %$hash) {
    my $entries = $hash->{$directory};
    die "mandatory directory is not a hash\n" unless ref($entries) eq 'HASH' && !tied(%$entries);
    my %out;
    for my $id (sort { $a <=> $b } keys %$entries) {
        die "mandatory tag id is not an unsigned 16-bit integer\n" unless $id =~ /^(?:0|[1-9][0-9]*)$/ && $id <= 65535;
        $out{"$id"} = scalar_or_die($entries->{$id});
    }
    $mandatory_map{$directory} = \%out;
}
my @row_properties = qw(Writable Format WriteGroup CanCreate DelValue Deletable PrintConvInv RawConvInv Validate ValueConvInv WriteAlso WriteCheck WriteCondition WriteHook WriteLast WritePseudo);
my %raw_rows;
for my $id (sort { $a <=> $b } grep { /^(?:0|[1-9][0-9]*)$/ && $_ <= 65535 } keys %Image::ExifTool::Exif::Main) {
    my $entry = $Image::ExifTool::Exif::Main{$id};
    next unless ref($entry) eq 'HASH';
    my %properties;
    for my $property (@row_properties) {
        if (exists $entry->{$property} && defined $entry->{$property}) {
            # A reference changes the native path and is deliberately not
            # translated by this bounded numeric-default carrier.
            $properties{$property} = ref($entry->{$property}) ? { present => JSON::PP::true, unsupported => JSON::PP::true }
                                                       : { present => JSON::PP::true, value => "$entry->{$property}" };
        } else {
            $properties{$property} = { present => JSON::PP::false };
        }
    }
    $raw_rows{"$id"} = \%properties;
}
my %closure;
for my $name (sort keys %INC) {
    next unless $name =~ m{^Image/ExifTool(?:/|\.pm$)};
    my ($absolute, $relative) = rel_source($INC{$name});
    open my $fh, '<:raw', $absolute or die "cannot read loaded native module\n";
    local $/;
    $closure{$relative} = sha256_hex(<$fh>);
}
die "WriteExif source is absent from selected module closure\n"
    unless $closure{$source_file} && $closure{$source_file} eq $source_sha256;
print JSON::PP->new->utf8->canonical->pretty->encode({
    schema => 1,
    kind => 'oxidex_exif_mandatory_defaults_fact',
    writer => {
        requested_binding => 'Image::ExifTool::Exif::WriteExif',
        actual_name => $actual_name,
        source_file => $source_file,
        source_sha256 => $source_sha256,
        cv_file => $source_file,
    },
    native_identity => {
        exiftool_version => "$Image::ExifTool::VERSION",
        perl => $^X,
        perl_version => "$^V",
    },
    lexical => { name => '%mandatory', resolved => JSON::PP::true, entries => \%mandatory_map },
    # Raw final-loaded Exif::Main entries selected by WriteExif via
    # `$tagTablePtr->{id}`.  These are deliberately not called effective
    # GetTagInfo projections: reference/inherited entries are captured as
    # unsupported and the bounded compiler refuses them.
    raw_exif_main_row_properties => \%raw_rows,
    loaded_exiftool_closure => \%closure,
    new_directory_context_deparse => $context,
});
