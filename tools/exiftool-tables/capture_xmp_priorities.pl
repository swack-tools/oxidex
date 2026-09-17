#!/usr/bin/env perl
#
# Record what FoundXMP (XMP.pm ~3437-3640) and FoundTag (ExifTool.pm
# ~9468-9590) need to give an XMP property its priority on the READ path.
#
# FoundXMP builds a tag ID from the property path (GetXMPTagID), selects the
# tag table by the first property's namespace prefix after %stdXlatNS (a key
# of %Image::ExifTool::XMP::Main), and looks the ID up case-sensitively
# (GetTagInfo). On a miss it flattens the first containing structure and
# retries; for a field of a variable-namespace structure (Struct with
# NAMESPACE => undef) it copies the field's tagInfo from the field's own
# namespace table; otherwise it mints { Name => ..., Priority => 0 }.
# FoundTag then takes Priority, else the tagInfo's Table PRIORITY, else 0 for
# Avoid, else 1.
#
# Recorded per Main key: the table, its NAMESPACE, PRIORITY and AVOID, and per
# raw tag ID (after Image::ExifTool::XMP::AddFlattenedTags on the whole table):
#   name          the tag Name (for reference only)
#   priority      the effective FoundTag priority of a direct hit
#   own_priority  the tagInfo's own Priority (null when undefined)
#   avoid         the tagInfo's Avoid after table setup (null when undefined)
#   struct        none | fixed | variable (Struct whose NAMESPACE exists and is undef)
# plus %xmpNS (ExifTool group prefix -> standard XMP prefix).
#
# Usage: capture_xmp_priorities.pl <exiftool-lib>
use strict;
use warnings;
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
use JSON::PP ();

my $lib = shift @ARGV;
die "usage: capture_xmp_priorities.pl <exiftool-lib>\n" unless defined $lib && !@ARGV;
unshift @INC, $lib;
require Image::ExifTool;
require Image::ExifTool::XMP;
no warnings 'once';

sub num { my $v = shift; defined $v ? 0 + $v : undef }
sub flag { my $v = shift; defined $v ? ($v ? JSON::PP::true : JSON::PP::false) : undef }

my $main = Image::ExifTool::GetTagTable('Image::ExifTool::XMP::Main');
my %namespaces;
for my $key (sort(Image::ExifTool::TagTableKeys($main))) {
    my $info = $$main{$key};
    next unless ref $info eq 'HASH' and $$info{SubDirectory} and $$info{SubDirectory}{TagTable};
    my $name = $$info{SubDirectory}{TagTable};
    my $table = Image::ExifTool::GetTagTable($name) or die "cannot load $name\n";
    # FoundXMP keys tags by bare ID only in a table with a true NAMESPACE.
    die "$name has no NAMESPACE\n" unless $$table{NAMESPACE};
    Image::ExifTool::XMP::AddFlattenedTags($table);
    my %tags;
    for my $id (sort(Image::ExifTool::TagTableKeys($table))) {
        my @infos = grep { ref $_ eq 'HASH' } Image::ExifTool::GetTagInfoList($table, $id);
        next unless @infos;
        my %fact;
        for my $tagInfo (@infos) {
            my $priority = $$tagInfo{Priority};
            unless (defined $priority) {
                $priority = $$tagInfo{Table}{PRIORITY};
                $priority = 0 if not defined $priority and $$tagInfo{Avoid};
            }
            $priority = 1 unless defined $priority;
            my $struct = $$tagInfo{Struct};
            my $kind = !$struct ? 'none'
                : (ref $struct eq 'HASH' and exists $$struct{NAMESPACE} and not defined $$struct{NAMESPACE})
                    ? 'variable' : 'fixed';
            my %this = (
                name => $$tagInfo{Name},
                priority => 0 + $priority,
                own_priority => num($$tagInfo{Priority}),
                avoid => flag($$tagInfo{Avoid}),
                struct => $kind,
            );
            my $json = JSON::PP->new->canonical;
            die "$name $id: conditional entries differ\n"
                if %fact and $json->encode(\%fact) ne $json->encode(\%this);
            %fact = %this;
        }
        $tags{$id} = \%fact;
    }
    die "Main key $key maps to two tables\n" if exists $namespaces{$key};
    $namespaces{$key} = {
        table => $name,
        namespace => $$table{NAMESPACE},
        table_priority => num($$table{PRIORITY}),
        table_avoid => flag($$table{AVOID}),
        tags => \%tags,
    };
}
my %xmpNS;
{
    # %xmpNS is a file lexical in XMP.pm, unreachable from outside the
    # module, so read its literal definition from the loaded source.
    my $path = $INC{'Image/ExifTool/XMP.pm'};
    open my $fh, '<', $path or die "$path: $!\n";
    local $/;
    my $src = <$fh>;
    $src =~ /^my %xmpNS = \((.*?)^\);/ms or die "cannot find %xmpNS in $path\n";
    my $body = $1;
    $body =~ s/#.*$//mg;
    while ($body =~ /'([^']+)'\s*=>\s*'([^']+)'/g) { $xmpNS{$1} = $2; }
    die "empty %xmpNS\n" unless %xmpNS;
}
print JSON::PP->new->canonical->pretty->utf8->encode({
    exiftool_version => "$Image::ExifTool::VERSION",
    namespaces => \%namespaces,
    xmp_ns => \%xmpNS,
});
