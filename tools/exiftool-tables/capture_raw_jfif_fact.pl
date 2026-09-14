#!/usr/bin/env perl
# Snapshot final native raw JFIF table/control inputs, never displayed values.
use strict;
use warnings;
use B;
use B::Deparse;
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
require 'Image/ExifTool/Writer.pl';
require 'Image/ExifTool/Exif.pm';
require 'Image/ExifTool/WriteExif.pl';
# Match the selected writer's post-load prototype/binding context. A later
# module replacing a core callback must be captured and refused, not hidden.
require 'Image/ExifTool/Canon.pm';
require 'Image/ExifTool/PanasonicRaw.pm';
require 'Image/ExifTool/Sony.pm';
sub source {
    my ($file) = @_;
    my $path = abs_path($file) // die "native source unavailable\n";
    die "native source outside selected library\n" unless index($path, "$lib/") == 0;
    open my $fh, '<:raw', $path or die "cannot read native source\n";
    local $/; my $bytes=<$fh>;
    return {file=>substr($path,length($lib)+1),sha256=>sha256_hex($bytes)};
}
my $deparser=B::Deparse->new('-p','-sC');
sub code_fact {
    my ($cv) = @_;
    return {resolved=>JSON::PP::false} unless ref($cv) eq 'CODE';
    my $object=B::svref_2object($cv);
    return {resolved=>JSON::PP::true,name=>subname($cv),source=>source($object->FILE),body=>$deparser->coderef2text($cv)};
}
sub plain {
    my ($value)=@_;
    return $value unless ref($value);
    return {code=>code_fact($value)} if ref($value) eq 'CODE';
    return [map {plain($_)} @$value] if ref($value) eq 'ARRAY';
    return {map {$_=>plain($value->{$_})} keys %$value} if ref($value) eq 'HASH';
    return {unsupported=>ref($value)};
}
sub hashes {
    my ($cv)=@_;
    my @pad=B::svref_2object($cv)->PADLIST->ARRAY;
    my @names=$pad[0]->ARRAY; my @values=$pad[1]->ARRAY;
    my %result;
    for my $i (0..$#names) {
        my $name=eval {$names[$i]->PV};
        next unless defined $name && $name =~ /^%/;
        die "ambiguous lexical hash\n" if exists $result{$name};
        my $hash=eval {$values[$i]->isa('B::HV') ? $values[$i]->object_2svref : undef};
        $result{$name}=ref($hash) eq 'HASH' && !tied(%$hash) ? plain($hash) : {unresolved=>JSON::PP::true};
    }
    return \%result;
}
my %functions;
for my $name (qw(WriteJPEG WriteDirectory WriteBinaryData ProcessDirectory ProcessBinaryData ReadValue FoundTag GetTagInfo GetTagTable SetupTagTable AddTagToTable TagTableKeys SetByteOrder Get8u Get16u DoUnpackStd)) {
    no strict 'refs'; my $binding="Image::ExifTool::$name";
    my $cv=*{$binding}{CODE} or die "missing native binding $name\n";
    $functions{$name}=code_fact($cv);
    $functions{$name}{requested_binding}=$binding;
    $functions{$name}{lexical_hashes}=hashes($cv) if $name eq 'ReadValue' || $name eq 'ProcessBinaryData' || $name eq 'SetByteOrder';
}
my %closure;
for my $name (sort keys %INC) {
    next unless $name =~ m{^Image/ExifTool(?:/|\.pm$)};
    my $fact=source($INC{$name}); $closure{$fact->{file}}=$fact->{sha256};
}
print JSON::PP->new->canonical->utf8->pretty->encode({
 schema=>1,kind=>'raw_jfif_native_fact',
 native_identity=>{perl=>$^X,perl_version=>"$^V",exiftool_version=>"$Image::ExifTool::VERSION"},
 loaded_closure=>\%closure,functions=>\%functions,
 table=>{binding=>'Image::ExifTool::JFIF::Main',source=>source($INC{'Image/ExifTool.pm'}),entries=>plain(\%Image::ExifTool::JFIF::Main)},
});
