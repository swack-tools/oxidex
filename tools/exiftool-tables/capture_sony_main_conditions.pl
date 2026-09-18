#!/usr/bin/env perl
# Evaluate `Sony::Main` model Conditions with a release's own ExifTool, in-process.
#
# `gen_sony_main_extra_tables.py` translates each `$$self{Model} =~ /RE/`
# Condition into an `MCond::ModelRe` the Rust interpreter evaluates with the
# `regex` crate. A Condition decides WHICH cameras get a tag, so a translation
# is only credible if it selects exactly the models the release's own Perl
# selects. This script is the Perl half of that proof; it never reads a
# version label to decide anything.
#
#   capture_sony_main_conditions.pl models <lib>
#       Print (JSON) the model strings named by <lib>'s in-memory
#       `%Image::ExifTool::Sony::Main` 0xb001 SonyModelID PrintConv.
#
#   capture_sony_main_conditions.pl eval <lib> <models.json> <tag-id>...
#       For every variant of every listed tag id whose Condition is exactly one
#       `$$self{Model} =~ /RE/` or `!~` test, `eval` that Condition string the
#       way ExifTool does (with `$self` a hash holding Model) over every model,
#       and print (JSON) the models it selects. Conditions of any other shape
#       are listed as `skipped` with their text, never evaluated; an id the
#       release does not define is listed in `absent_ids`.
use strict;
use warnings;
use JSON::PP;
use Digest::SHA qw(sha256_hex);

my ($mode, $lib, @rest) = @ARGV;
die "usage: $0 models <lib> | eval <lib> <models.json> <tag-id>...\n"
    unless defined $lib and ($mode // '') =~ /^(models|eval)$/;
die "$lib/Image/ExifTool/Sony.pm: not found\n" unless -f "$lib/Image/ExifTool/Sony.pm";
unshift @INC, $lib;
require Image::ExifTool;
require Image::ExifTool::Sony;
for my $mod ('Image/ExifTool.pm', 'Image/ExifTool/Sony.pm') {
    die "$mod loaded from $INC{$mod}, not $lib\n" unless $INC{$mod} eq "$lib/$mod";
}
my $main = do { no warnings "once"; \%Image::ExifTool::Sony::Main };
my $json = JSON::PP->new->canonical->pretty;

if ($mode eq 'models') {
    my $pc = $$main{0xb001}{PrintConv};
    die "Sony::Main 0xb001 has no PrintConv hash\n" unless ref $pc eq 'HASH';
    my %seen;
    for my $name (values %$pc) {
        # `ILCE-3000 / ILCE-3500`, `DSLR-A380/A390`, `DSLR-A850 (APS-C mode)`:
        # one PrintConv value can name several bodies; each is a Model string.
        (my $base = $name) =~ s/\s*\(.*\)\s*$//;
        my @parts = split m{\s*/\s*}, $base;
        my ($prefix) = $parts[0] =~ /^([A-Z]+-)/;
        for my $p (@parts) {
            $p = ($prefix // '') . $p unless $p =~ /-/;
            $seen{$p} = 1;
        }
    }
    print $json->encode({
        source => 'Sony::Main 0xb001 SonyModelID PrintConv',
        sony_pm_sha256 => sha256_file("$lib/Image/ExifTool/Sony.pm"),
        exiftool_version => "$Image::ExifTool::VERSION",
        models => [ sort keys %seen ],
    });
    exit 0;
}

my ($models_path, @ids) = @rest;
die "no tag ids\n" unless @ids;
my $models = decode_json(do { local (@ARGV, $/) = $models_path; <> });
die "$models_path: expected a JSON list of strings\n" unless ref $models eq 'ARRAY';

my (@conditions, @skipped, @absent);
for my $id_text (@ids) {
    my $id = $id_text =~ /^0x/i ? hex $id_text : $id_text;
    my $entry = $$main{$id};
    unless (defined $entry) {
        # Not this proof's concern: the generator refuses a missing id itself.
        push @absent, sprintf('0x%x', $id);
        next;
    }
    my @variants = ref $entry eq 'ARRAY' ? @$entry : ($entry);
    for my $i (0 .. $#variants) {
        my $v = $variants[$i];
        next unless ref $v eq 'HASH' and defined $$v{Condition};
        my $cond = $$v{Condition};
        my %row = (tag => sprintf('0x%x', $id), variant => $i, name => $$v{Name}, condition => $cond);
        unless ($cond =~ m{^\s*\$\$self\{Model\}\s*[=!]~\s*/[^/]*/\s*$}) {
            push @skipped, \%row;
            next;
        }
        my @selected;
        for my $model (@$models) {
            my $self = { Model => $model };
            my $hit = eval $cond;
            die "$row{tag} $$v{Name}: Condition died: $@" if $@;
            push @selected, $model if $hit;
        }
        $row{selects} = \@selected;
        push @conditions, \%row;
    }
}
print $json->encode({
    exiftool_version => "$Image::ExifTool::VERSION",
    sony_pm_sha256 => sha256_file("$lib/Image/ExifTool/Sony.pm"),
    perl => sprintf('%vd', $^V),
    conditions => \@conditions,
    skipped => \@skipped,
    absent_ids => \@absent,
});

sub sha256_file {
    my ($path) = @_;
    open my $fh, '<:raw', $path or die "$path: $!\n";
    local $/;
    return sha256_hex(<$fh>);
}
