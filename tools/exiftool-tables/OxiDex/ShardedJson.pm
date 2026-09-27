package OxiDex::ShardedJson;
use strict;
use warnings;
use JSON::PP ();
use Scalar::Util qw(refaddr);

# Encode one shard of a dump_tables.pl document as mergeable framed records.
#
# Every shard process performs the complete capture in the same order, so
# global facts (loaded closures, source hashes, runtime links, hydrated object
# numbering) cannot drift between shards.  dump_tables_shard.pl divides only two
# pure computations, and names the affected subtrees by reference address:
#
#   owners  -- {refaddr => shard}: a subtree only that shard computed
#              faithfully (a write-sidecar unit).  It is always its own leaf
#              and only its owner emits it; other shards hold a placeholder.
#   unions  -- {refaddr => 1}: a hash each shard holds only part of (hydrated
#              Validate names).  Every shard emits its entries as a U record;
#              the merge unions them, requiring equal values for equal keys.
#
# The document is split into a skeleton plus leaf values.  The skeleton is
# the exact text JSON::PP->new->utf8->canonical->pretty would print around the
# leaves; each leaf is that encoder's own output for the leaf value,
# re-indented to its depth.  Every shard streams the whole skeleton and only
# its own leaves as framed records, in document order:
#
#     H <shard> <count>\n                           once, first
#     S <seq> <bytes>\n<bytes>                      skeleton text (all shards)
#     L <seq> <bytes>\n<bytes>                      leaf text (owning shard)
#     U <seq> <bytes>\n<bytes>                      union-map part (all shards):
#         "<level>\n" then per entry "<keylen> <textlen> <valuelen>\n"
#         followed by the UTF-8 key, its JSON text, and its value text
#     E <records>\n                                 once, last
#
# merge_dump_shards.py interleaves records by <seq>, requires every S record
# to be byte-identical across all shards (that is the global-facts agreement
# check), every L record to come from exactly one shard, and unions every U
# record.  A subtree is split when it contains an owned or union node, or
# when its node count exceeds total/(16 * shards); every other leaf goes to
# the shard with the least node count so far.  Owned and union contents count
# as zero, so costs, splits and the partition are identical in every shard
# and no shard needs to hold a plan: records are written as they are found.

my $INDENT = '   ';

our ($OWNERS, $UNIONS) = ({}, {});

sub new_encoder { JSON::PP->new->utf8->canonical->pretty->allow_nonref }

sub _special {
    my ($value) = @_;
    my $id = ref($value) ? refaddr($value) : undef;
    return defined($id) && (exists $OWNERS->{$id} || exists $UNIONS->{$id});
}

# (node count, contains-special) of a subtree.  Only nodes containing a
# special node, or above the split threshold once it is known, are memoized:
# those are the ones the planner revisits, and a small subtree is cheap to
# count again.  This keeps the memo far smaller than one entry per node.
sub _measure {
    my ($state, $value) = @_;
    my $type = ref $value;
    return (1, 0) unless $type eq 'HASH' || $type eq 'ARRAY';
    return (0, 1) if _special($value);
    my $id = refaddr($value);
    if (my $hit = $state->{memo}{$id}) { return @$hit }
    my $cost = 1 + ($type eq 'HASH' ? 2 * keys(%$value) : 0);
    my $tainted = 0;
    for my $child ($type eq 'HASH' ? values %$value : @$value) {
        my ($c, $t) = _measure($state, $child);
        $cost += $c;
        $tainted ||= $t;
    }
    $state->{memo}{$id} = [ $cost, $tainted ]
        if $tainted || (defined $state->{threshold} && $cost > $state->{threshold});
    return ($cost, $tainted);
}

sub _key_text {
    my ($encoder, $key) = @_;
    my $text = $encoder->encode($key);
    $text =~ s/\n\z//;
    return $text;
}

sub _leaf_text {
    my ($encoder, $value, $level) = @_;
    my $text = $encoder->encode($value);
    $text =~ s/\n\z//;
    if ($level) {
        my $pad = $INDENT x $level;
        $text =~ s/\n/\n$pad/g;
    }
    return $text;
}

sub _union_text {
    my ($encoder, $hash, $level) = @_;
    my $body = "$level\n";
    for my $key (sort { $a cmp $b } keys %$hash) {
        my $raw = $key;
        utf8::encode($raw);
        my $text = _key_text($encoder, $key);
        my $value = _leaf_text($encoder, $hash->{$key}, $level + 1);
        $body .= length($raw) . ' ' . length($text) . ' ' . length($value) . "\n"
            . $raw . $text . $value;
    }
    return $body;
}

sub _frame {
    my ($state, $kind, $bytes) = @_;
    my $fh = $state->{fh};
    print {$fh} "$kind $state->{seq} " . length($bytes) . "\n", $bytes;
    ++$state->{seq};
}

sub _flush_skeleton {
    my ($state) = @_;
    return unless length $state->{skeleton};
    _frame($state, 'S', $state->{skeleton});
    $state->{skeleton} = '';
}

# A leaf slot: the owner writes it, every other shard only advances <seq>.
sub _leaf {
    my ($state, $owner, $value, $level) = @_;
    _flush_skeleton($state);
    if ($owner == $state->{index}) {
        _frame($state, 'L', _leaf_text($state->{encoder}, $value, $level));
    } else {
        ++$state->{seq};
    }
}

sub _emit {
    my ($state, $value, $level) = @_;
    my $type = ref $value;
    if (_special($value)) {
        my $id = refaddr($value);
        if (exists $OWNERS->{$id}) {
            _leaf($state, $OWNERS->{$id}, $value, $level);
        } else {
            die "union node must be a hash\n" unless $type eq 'HASH';
            _flush_skeleton($state);
            _frame($state, 'U', _union_text($state->{encoder}, $value, $level));
        }
        return;
    }
    my $splittable = ($type eq 'HASH' && %$value) || ($type eq 'ARRAY' && @$value);
    my ($cost, $tainted) = _measure($state, $value);
    if (!$splittable || (!$tainted && $cost <= $state->{threshold})) {
        my $load = $state->{load};
        my $best = 0;
        for my $s (1 .. $#$load) { $best = $s if $load->[$s] < $load->[$best] }
        $load->[$best] += $cost;
        _leaf($state, $best, $value, $level);
        return;
    }
    my $inner = $INDENT x ($level + 1);
    my ($open, $close) = $type eq 'HASH' ? ('{', '}') : ('[', ']');
    $state->{skeleton} .= "$open\n";
    my @items = $type eq 'HASH' ? (sort { $a cmp $b } keys %$value) : (0 .. $#$value);
    for my $i (0 .. $#items) {
        $state->{skeleton} .= $i ? ",\n$inner" : $inner;
        $state->{skeleton} .= _key_text($state->{encoder}, $items[$i]) . ' : ' if $type eq 'HASH';
        _emit($state, $type eq 'HASH' ? $value->{$items[$i]} : $value->[$items[$i]], $level + 1);
    }
    $state->{skeleton} .= "\n" . ($INDENT x $level) . $close;
}

# Write shard $index of $count for $document to $fh.
sub write_shard {
    my ($fh, $document, $index, $count, %options) = @_;
    die "invalid shard $index/$count\n"
        unless $count >= 1 && $index >= 0 && $index < $count;
    local $OWNERS = $options{owners} || {};
    local $UNIONS = $options{unions} || {};
    binmode($fh, ':raw');
    my %state = (fh => $fh, index => $index, encoder => new_encoder(),
                 memo => {}, threshold => undef, seq => 0, skeleton => '',
                 load => [ (0) x $count ]);
    # The sizing pass fixes the threshold: ~16 leaves per shard keeps the
    # greedy partition balanced without shredding the skeleton.
    my ($total) = _measure(\%state, $document);
    $state{threshold} = int($total / ($count * 16)) || 1;
    print {$fh} "H $index $count\n";
    _emit(\%state, $document, 0);
    $state{skeleton} .= "\n";    # JSON::PP pretty ends the document with a newline
    _flush_skeleton(\%state);
    print {$fh} "E $state{seq}\n";
}

1;
