#!/usr/bin/env perl
# Export precisely the seven hashes selected by XMP.pm::PrintLensID. The
# consumer generates Rust, but this process alone loads the selected Perl tree.
use strict;
use warnings;
BEGIN {
    for my $name (qw(PERL5OPT PERL5LIB PERLLIB)) {
        die "refusing ambient $name\n" if defined $ENV{$name} && length $ENV{$name};
    }
}
use B ();
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use Encode qw(decode FB_CROAK);
use FindBin;
use Getopt::Long qw(GetOptions);
use JSON::PP ();

my $root = abs_path("$FindBin::Bin/../..");
my $tree;
GetOptions('exiftool-dir=s' => \$tree) or die "usage: $0 --exiftool-dir TREE\n";
die "usage: $0 --exiftool-dir TREE\n" unless defined $tree && !@ARGV;
$tree = abs_path($tree) or die "selected ExifTool tree does not exist\n";
my $lib = "$tree/lib";
open my $fh, '<', "$root/.exiftool-version" or die "cannot read version pin: $!\n";
my $pin = do { local $/; <$fh> };
close $fh;
$pin =~ s/\s+\z//;
die "invalid version pin\n" unless $pin =~ /^\d+\.\d+\z/;

my @spec = (
    [Canon   => 'Canon',    'canonLensTypes'],
    [Nikon   => 'Nikon',    'nikonLensIDs'],
    [Pentax  => 'Pentax',   'pentaxLensTypes'],
    [Sony    => 'Sony',     'sonyLensTypes'],
    [Sigma   => 'SigmaRaw', 'sigmaLensTypes'],
    [Samsung => 'Samsung',  'samsungLensTypes'],
    [Leica   => 'Panasonic','leicaLensTypes'],
);
my @modules = ('Image/ExifTool.pm', 'Image/ExifTool/XMP.pm', 'Image/ExifTool/Exif.pm',
               'Image/ExifTool/Sigma.pm',
               map { "Image/ExifTool/$_->[1].pm" } @spec);
my (%seen, %source);
unshift @INC, $lib;
for my $module (@modules) {
    next if $seen{$module}++;
    my $expected = abs_path("$lib/$module") or die "missing selected module $module\n";
    require $module;
    my $loaded = defined($INC{$module}) ? abs_path($INC{$module}) : undef;
    die "$module loaded outside selected tree\n"
        unless defined($loaded) && $loaded eq $expected;
    open my $in, '<:raw', $expected or die "cannot read $expected: $!\n";
    $source{$module} = { path => $expected, sha256 => sha256_hex(do { local $/; <$in> }) };
    close $in;
    if ($module =~ m{^Image/ExifTool/(\w+)\.pm$}) {
        no strict 'refs';
        my $ver = ${"Image::ExifTool::$1\::VERSION"};
        use strict 'refs';
        die "$module has no module version\n" unless defined($ver) && $ver =~ /^\d+\.\d+\z/;
        $source{$module}{version} = $ver;
    }
}
die "selected ExifTool $Image::ExifTool::VERSION disagrees with pin $pin\n"
    unless defined($Image::ExifTool::VERSION) && $Image::ExifTool::VERSION eq $pin;
open my $xmp_fh, '<:raw', "$lib/Image/ExifTool/XMP.pm" or die "cannot read XMP.pm: $!\n";
my $xmp_source = do { local $/; <$xmp_fh> };
close $xmp_fh;
for my $fragment ('qw(Canon Nikon Pentax Sony Sigma Samsung Leica)',
                  "{ Sigma => 'SigmaRaw', Leica => 'Panasonic' }",
                  "{ Nikon => 'nikonLensIDs' }",
                  '%$convName or last',
                  'Image::ExifTool::Exif::PrintLensID($et, $str, $printConv,') {
    die "XMP.pm selected-table contract changed: $fragment\n"
        unless index($xmp_source, $fragment) >= 0;
}

sub text_value {
    my ($maker, $key, $value) = @_;
    die "$maker/$key: expected truthy string scalar\n"
        unless defined($value) && !ref($value)
            && (B::svref_2object(\$value)->FLAGS & B::SVp_POK()) && $value;
    return $value if utf8::is_utf8($value);
    my $copy = $value;
    my $utf8 = eval { decode('UTF-8', $copy, FB_CROAK) };
    return defined($utf8) ? $utf8 : decode('ISO-8859-1', $value);
}

my @makers;
for my $entry (@spec) {
    my ($maker, $module, $name) = @$entry;
    no strict 'refs';
    my $table = \%{"Image::ExifTool::$module\::$name"};
    use strict 'refs';
    die "$maker: lookup is not a hash\n" unless ref($table) eq 'HASH';
    my (@rows, @exceptions);
    for my $key (sort keys %$table) {
        if ($key eq 'Notes') {
            text_value($maker, $key, $table->{$key});
            push @exceptions, { key => $key, type => 'STRING', disposition => 'table documentation excluded' };
            next;
        }
        if ($key eq 'OTHER' && $maker =~ /^(?:Nikon|Pentax|Sony|Leica)$/) {
            die "$maker/OTHER: expected CODE exception\n" unless ref($table->{$key}) eq 'CODE';
            push @exceptions, { key => $key, type => 'CODE', disposition => 'source dynamic fallback excluded' };
            next;
        }
        my $ok = $maker eq 'Nikon'
            ? $key =~ /^[0-9A-F]{2}(?: [0-9A-F]{2})*(?:\.[1-9]\d*)?\z/
            : $maker eq 'Pentax'
            ? $key =~ /^\d+ \d+(?:\.[1-9]\d*)?\z/
            : $maker eq 'Leica'
            ? $key =~ /^\d+(?: \d+)?(?:\.[1-9]\d*)?\z/
            : $key =~ /^-?\d+(?:\.[1-9]\d*)?\z/;
        die "$maker: unsupported key '$key'\n" unless $ok;
        push @rows, [$key, text_value($maker, $key, $table->{$key})];
    }
    if ($maker eq 'Sigma') {
        # XMP.pm selects SigmaRaw::sigmaLensTypes even though SigmaRaw.pm's
        # PrintConv references Sigma::sigmaLensTypes. In 13.59 the selected
        # SigmaRaw hash is empty, so XMP.pm's `%$convName or last` stops here.
        die "selected $maker lookup is no longer empty; revisit adapter\n" if @rows || @exceptions;
    } else {
        die "$maker: selected lookup unexpectedly empty\n" unless @rows;
    }
    if ($maker =~ /^(?:Nikon|Pentax|Sony|Leica)$/) {
        die "$maker: OTHER exception missing\n" unless grep { $_->{key} eq 'OTHER' } @exceptions;
    } elsif (grep { $_->{key} eq 'OTHER' } @exceptions) {
        die "$maker: unexpected OTHER exception\n";
    }
    # Exif.pm and Canon.pm visit .1, .2, ... until a false entry. Nikon is
    # different: Nikon.pm intentionally has orphan .N suffixes, and XMP.pm
    # scans all matching prefixes into a fresh, dense map before delegation.
    my %base = map { $_->[0] => 1 } grep { $_->[0] !~ /\.[1-9]\d*\z/ } @rows;
    my %fractions;
    for my $row (@rows) {
        next unless $row->[0] =~ /^(.*)\.([1-9]\d*)\z/;
        push @{$fractions{$1}}, 0 + $2;
    }
    for my $key (keys %fractions) {
        die "$maker: orphan fractional chain $key\n" unless $base{$key} || $maker eq 'Nikon';
        my @indices = sort { $a <=> $b } @{$fractions{$key}};
        for my $i (0 .. $#indices) {
            die "$maker: noncontiguous fractional chain $key\n" unless $indices[$i] == $i + 1;
        }
    }
    push @makers, { make => $maker, module => $module, table => $name,
                    rows => \@rows, exceptions => \@exceptions,
                    base_count => scalar(keys %base),
                    fractional_count => scalar(@rows) - scalar(keys %base) };
}
binmode STDOUT, ':encoding(UTF-8)';
print JSON::PP->new->canonical->encode({ schema => 'xmp_lens_maps_v1',
    exiftool_version => $pin, source => \%source, makers => \@makers });
