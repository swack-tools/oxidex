# The IsOffset tags of every tag table the pinned ExifTool loads, with the
# length tag each one is paired with. Output lines:
#   KIND <tab> OFFSET_NAME <tab> LENGTH_NAME <tab> TABLES
# KIND: offsetpair -- IsOffset, and its OffsetPair (or, without one, a tag of
# the same table named <stem>Length/ByteCount/ByteCounts/Size, as the
# ProcessBinaryData thumbnail tables pair them) names the length;
# unpaired -- IsOffset with no length tag at all.
# usage: perl -I<pinned lib> makernote_offset_pairs.pl
# (driven by makernote_offset_pairs.py; see its docstring.)
use strict; use warnings;
use Image::ExifTool;
my $lib = $INC{'Image/ExifTool.pm'}; $lib =~ s{/Image/ExifTool\.pm$}{};
opendir my $d, "$lib/Image/ExifTool" or die; my @mods = sort grep s/\.pm$//, readdir $d;
# A module that cannot load (a dependency the selected Perl lacks) would drop
# every pair it declares from the inventory while the walk still succeeds:
# abort instead, naming the module and Perl's error.
for my $m (@mods) { eval "require Image::ExifTool::$m; 1" or die "cannot load Image::ExifTool::$m: $@"; }
no strict 'refs';
my @pkgs = ('Image::ExifTool', map { "Image::ExifTool::$_" } sort grep { /^\w+$/ } map { my $x = $_; $x =~ s/::$//; $x } grep /::$/, keys %{"Image::ExifTool::"});
my %out;
for my $p (@pkgs) {
  for my $sym (sort keys %{"${p}::"}) {
    next if $sym =~ /::$/;
    my $h = \%{"${p}::$sym"}; next unless %$h and ($h->{GROUPS} or $h->{PROCESS_PROC} or $h->{WRITE_PROC});
    my $tn = "${p}::$sym";
        my $infos = sub { my $v = shift; ref $v eq 'HASH' ? ($v) : ref $v eq 'ARRAY' ? grep { ref $_ eq 'HASH' } @$v : () };
    my %names;
    for my $k (keys %$h) {
      next if $k =~ /^[A-Z_]+$/;
      # a plain string value is the tag's name (`3 => 'ThumbnailLength'`)
      if (defined $h->{$k} and !ref $h->{$k}) { $names{$h->{$k}} = 1; next; }
      for my $i ($infos->($h->{$k})) { $names{$i->{Name}} = 1 if $i->{Name}; }
    }
    for my $k (sort keys %$h) {
      next if $k =~ /^[A-Z_]+$/;
      for my $i ($infos->($h->{$k})) {
        my $flags = $i->{Flags} ? (ref $i->{Flags} ? $i->{Flags} : [$i->{Flags}]) : [];
        my $iso = $i->{IsOffset} || grep { $_ eq 'IsOffset' } @$flags;
        my $name = $i->{Name} or next;
        if ($iso) {
          my @len;
          if (defined $i->{OffsetPair} and exists $h->{$i->{OffsetPair}}) {
            @len = grep { defined $_ and $_ =~ /(Length|ByteCounts?|Size)$/ } map { $_->{Name} } $infos->($h->{$i->{OffsetPair}});
          }
          if (!@len) {
            (my $stem = $name) =~ s/(Offsets?|Start)$//;
            @len = grep { $names{$_} } map { "$stem$_" } qw(Length ByteCount ByteCounts Size);
          }
          if (@len) { $out{"offsetpair\t$name\t$_"}{$tn} = 1 for @len } else { $out{"unpaired\t$name\t"}{$tn} = 1 }
        }
      }
    }
  }
}
print "$_\t", join(',', sort keys %{$out{$_}}), "\n" for sort keys %out;
