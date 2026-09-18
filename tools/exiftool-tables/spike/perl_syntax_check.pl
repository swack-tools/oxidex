#!/usr/bin/env perl
# SPIKE (measurement only): independent cross-check of the spike parser.
#
# Reads a JSON array of {id, form, text} on the path given in $ARGV[0] and
# reports, for each, whether PERL ITSELF can COMPILE the expression as a sub
# body -- `eval "sub { ... }"`, which compiles and never runs it. This is the
# instrument that keeps `perl_subset.py`'s refusals honest: a refusal Perl
# also rejects is ExifTool's own broken source, while a refusal Perl accepts
# is a hole in the spike grammar, and the two are indistinguishable from
# inside the parser (AGENTS.md, "Name the instrument").
#
# Nothing is executed: `eval STRING` on a `sub { ... }` wrapper compiles the
# body and returns the code ref, which is discarded. No ExifTool module is
# loaded and no file is read.
use strict;
use warnings;
use JSON::PP;

my $path = shift or die "usage: $0 <exprs.json>\n";
open my $fh, '<:raw', $path or die "open $path: $!";
my $json = do { local $/; <$fh> };
close $fh;
my $items = JSON::PP->new->decode($json);

my %out;
for my $item (@$items) {
    my $text = $item->{text};
    my $src = $item->{form} eq 'code' ? "sub $text" : "sub { $text\n }";
    local $@;
    my $ok = do {
        no strict;    ## no critic
        no warnings;
        eval $src;
    };
    $out{ $item->{id} } = defined($ok) && !$@ ? undef : do {
        my $err = $@ || 'unknown compile failure';
        $err =~ s/\s+/ /g;
        substr($err, 0, 200);
    };
}
print JSON::PP->new->canonical->encode(\%out);
