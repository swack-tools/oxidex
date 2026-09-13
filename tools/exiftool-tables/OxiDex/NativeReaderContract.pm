package OxiDex::NativeReaderContract;

use strict;
use warnings;
use B ();
use B::Deparse ();
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use Exporter qw(import);
use File::Spec ();
use File::Temp qw(tempfile);
use POSIX qw(WNOHANG);
use Time::HiRes qw(time sleep);
use JSON::PP;

our @EXPORT_OK = qw(capture_in_process capture_isolated_contract finalise_loaded_contract);

sub code_name {
    my ($cv) = @_;
    my $b = eval { B::svref_2object($cv) } or return undef;
    return undef unless $b->isa('B::CV');
    my $gv = eval { $b->GV } or return undef;
    return undef if ref($gv) eq 'B::SPECIAL';
    my $stash = eval { $gv->STASH->NAME } // '';
    my $name = eval { $gv->NAME } // '';
    return undef unless $name;
    return $stash ? "${stash}::${name}" : $name;
}

sub unresolved_code_fact {
    my ($name, $reason) = @_;
    return {
        __perl => 'CODE', __opaque => JSON::PP::true, __name => $name,
        resolved => JSON::PP::false, __deparse => undef,
        source_file => undef, source_sha256 => undef, reason => $reason,
    };
}

sub code_source_fact {
    my ($name, $lib_abs) = @_;
    return unresolved_code_fact($name, 'invalid_fully_qualified_name')
        unless $name =~ /^(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*$/;
    no strict 'refs';
    my $cv = *{$name}{CODE};
    return unresolved_code_fact($name, 'code_ref_unavailable') unless $cv;
    my $resolved_name = code_name($cv) // $name;
    my $body = eval { B::Deparse->new('-p', '-sC')->coderef2text($cv) };
    return unresolved_code_fact($resolved_name, 'deparse_unavailable')
        unless defined $body;
    my $file = eval { B::svref_2object($cv)->FILE };
    return unresolved_code_fact($resolved_name, 'source_file_unavailable')
        unless defined $file && length $file;
    my $abs = abs_path($file);
    return unresolved_code_fact($resolved_name, 'source_file_unreadable')
        unless defined $abs && -f $abs;
    my $prefix = $lib_abs . '/';
    return unresolved_code_fact($resolved_name, 'source_outside_selected_lib')
        unless index($abs, $prefix) == 0;
    open(my $fh, '<:raw', $abs)
        or return unresolved_code_fact($resolved_name, 'source_file_unreadable');
    local $/;
    my $bytes = <$fh>;
    close($fh)
        or return unresolved_code_fact($resolved_name, 'source_file_unreadable');
    return {
        __perl => 'CODE', __opaque => JSON::PP::true, __name => $resolved_name,
        resolved => JSON::PP::true, __deparse => $body,
        source_file => File::Spec->abs2rel($abs, $lib_abs),
        source_sha256 => sha256_hex($bytes),
    };
}

sub builtin_override_facts {
    my ($lib_abs) = @_;
    my %facts;
    for my $name (qw(unpack pack)) {
        my $fq_name = "CORE::GLOBAL::$name";
        no strict 'refs';
        my $cv = *{$fq_name}{CODE};
        $facts{$name} = $cv
            ? { present => JSON::PP::true, function => code_source_fact($fq_name, $lib_abs) }
            : { present => JSON::PP::false };
    }
    return \%facts;
}

sub builtin_overrides {
    my ($facts) = @_;
    return { map { $_ => $facts->{$_}{present} ? JSON::PP::true : JSON::PP::false }
        qw(unpack pack) };
}

sub loaded_state {
    no warnings 'once';
    return {
        reported_byte_order => $Image::ExifTool::currentByteOrder,
        unpack_std_s => $Image::ExifTool::unpackStd{'S'},
    };
}

sub observe_order {
    my ($order) = @_;
    my $ret = eval { Image::ExifTool::SetByteOrder($order) };
    return {
        set_return => defined $ret ? $ret : undef,
        reported_byte_order => eval { Image::ExifTool::GetByteOrder() },
        unpack_std_s => do { no warnings 'once'; $Image::ExifTool::unpackStd{'S'} },
        error => $@ || undef,
    };
}

sub hex_or_undef {
    my ($value) = @_;
    return undef unless defined $value;
    return sprintf('%x', $value);
}

sub get16u_observation {
    my ($order, $bytes, $offset, $expected, $expect_error) = @_;
    my ($actual, $error, $warning);
    {
        local $SIG{__WARN__} = sub { $warning .= shift };
        my $ok = eval {
            $actual = Image::ExifTool::Get16u(\$bytes, $offset);
            1;
        };
        $error = $@ unless $ok;
    }
    my $matches = $expect_error ? defined($error)
        : !defined($error)
            && ((!defined($expected) && !defined($actual))
                || (defined($expected) && defined($actual) && $expected == $actual));
    return ($matches, {
        byte_order => $order,
        offset => $offset,
        bytes_hex => unpack('H*', $bytes),
        expected => hex_or_undef($expected),
        expected_outcome => $expect_error ? 'error' : (defined($expected) ? 'value' : 'undef'),
        observed => hex_or_undef($actual),
        warning => $warning || undef,
        error => $error || undef,
    });
}

sub probe_get16u {
    my $initial = Image::ExifTool::GetByteOrder();
    my ($case_count, $failure_count) = (0, 0);
    my @failure_details;
    for my $order (qw(II MM)) {
        my $template = $order eq 'II' ? 'v' : 'n';
        Image::ExifTool::SetByteOrder($order);
        for my $offset (0, 2) {
            for my $value (0 .. 0xffff) {
                my $encoded = pack($template, $value);
                my $bytes = $offset ? "\xaa\xbb${encoded}\xcc\xdd" : "${encoded}\xcc\xdd";
                my $expected = unpack($template, $encoded);
                my ($matches, $detail) = get16u_observation(
                    $order, $bytes, $offset, $expected, 0);
                $case_count++;
                next if $matches;
                $failure_count++;
                push @failure_details, $detail if @failure_details < 32;
            }
        }
    }
    my @boundary_cases;
    for my $order (qw(II MM)) {
        Image::ExifTool::SetByteOrder($order);
        for my $case (
            [ 'empty_at_zero', '', 0, 0 ],
            [ 'one_byte_at_zero', "\x34", 0, 0 ],
            [ 'one_byte_remaining', "\xaa\xbb\x34", 2, 0 ],
            [ 'offset_at_end', "\xaa\xbb", 2, 0 ],
            [ 'offset_beyond_end', "\xaa\xbb", 3, 1 ],
        ) {
            my ($name, $bytes, $offset, $expect_error) = @$case;
            my ($matches, $detail) = get16u_observation(
                $order, $bytes, $offset, undef, $expect_error);
            $detail->{name} = $name;
            $detail->{matches_expected_native_outcome} = $matches ? JSON::PP::true : JSON::PP::false;
            push @boundary_cases, $detail;
            $case_count++;
            next if $matches;
            $failure_count++;
            push @failure_details, $detail if @failure_details < 32;
        }
    }
    my $restore_return = eval { Image::ExifTool::SetByteOrder($initial) };
    my $restore_error = $@ || undef;
    return {
        kind => 'get16u_native_probe_v1',
        valid_case_count => $case_count - scalar(@boundary_cases),
        boundary_case_count => scalar(@boundary_cases),
        failure_count => $failure_count,
        failure_details => \@failure_details,
        boundary_cases => \@boundary_cases,
        restore_return => defined $restore_return ? $restore_return : undef,
        restored_byte_order => eval { Image::ExifTool::GetByteOrder() },
        restore_error => $restore_error,
    };
}

sub capture_in_process {
    my ($lib_abs) = @_;
    my $initial = Image::ExifTool::GetByteOrder();
    my $state_at_entry = loaded_state();
    my %orders = map { $_ => observe_order($_) } qw(II MM);
    my $restore_return = eval { Image::ExifTool::SetByteOrder($initial) };
    my $restore_error = $@ || undef;
    my $override_facts = builtin_override_facts($lib_abs);
    return {
        kind => 'binary_unsigned_reader_contract_v1',
        exiftool_version => $Image::ExifTool::VERSION,
        loaded_functions => {
            get16u => code_source_fact('Image::ExifTool::Get16u', $lib_abs),
            do_unpack_std => code_source_fact('Image::ExifTool::DoUnpackStd', $lib_abs),
            set_byte_order => code_source_fact('Image::ExifTool::SetByteOrder', $lib_abs),
            get_byte_order => code_source_fact('Image::ExifTool::GetByteOrder', $lib_abs),
        },
        builtin_overrides => builtin_overrides($override_facts),
        builtin_override_facts => $override_facts,
        loaded_state => $state_at_entry,
        observations => {
            initial_byte_order => $initial,
            orders => \%orders,
            restore_return => defined $restore_return ? $restore_return : undef,
            restored_byte_order => eval { Image::ExifTool::GetByteOrder() },
            restore_error => $restore_error,
        },
        get16u_probe => probe_get16u(),
    };
}

sub unresolved_contract {
    my ($reason) = @_;
    return {
        kind => 'binary_unsigned_reader_contract_v1',
        resolved => JSON::PP::false, reason => $reason,
    };
}

sub capture_isolated_contract {
    my ($perl, $extractor, $lib, $timeout) = @_;
    return unresolved_contract('extractor_unavailable') unless -f $extractor;
    $timeout //= 60;
    return unresolved_contract('extractor_invalid_timeout')
        unless $timeout =~ /\A[1-9]\d*\z/;
    my @command = ($perl, $extractor, '--timeout', $timeout, $lib);
    my ($out_fh, $out_path) = tempfile('oxidex-reader-contract-out-XXXX',
        TMPDIR => 1, UNLINK => 1);
    my ($err_fh, $err_path) = tempfile('oxidex-reader-contract-err-XXXX',
        TMPDIR => 1, UNLINK => 1);
    close($out_fh) && close($err_fh)
        or return unresolved_contract('extractor_tempfile_close_failed');
    my $pid = fork();
    return unresolved_contract('extractor_spawn_failed') unless defined $pid;
    if (!$pid) {
        open(STDOUT, '>', $out_path) or exit 127;
        open(STDERR, '>', $err_path) or exit 127;
        exec @command;
        exit 127;
    }
    my $deadline = time() + $timeout;
    my $status;
    while (1) {
        my $done = waitpid($pid, WNOHANG);
        if ($done == $pid) {
            $status = $?;
            last;
        }
        if (time() >= $deadline) {
            kill 'TERM', $pid;
            sleep 0.05;
            kill 'KILL', $pid if waitpid($pid, WNOHANG) == 0;
            waitpid($pid, 0);
            return unresolved_contract('extractor_timeout');
        }
        sleep 0.01;
    }
    return unresolved_contract('extractor_failed') unless $status == 0;
    open(my $fh, '<:raw', $out_path)
        or return unresolved_contract('extractor_output_unreadable');
    local $/;
    my $output = <$fh>;
    close($fh) or return unresolved_contract('extractor_output_unreadable');
    return unresolved_contract('extractor_empty_output') unless defined $output;
    my $contract = eval { JSON::PP::decode_json($output) };
    return unresolved_contract('extractor_invalid_json') unless ref($contract) eq 'HASH';
    return unresolved_contract('extractor_invalid_shape')
        unless ($contract->{kind} // '') eq 'binary_unsigned_reader_contract_v1'
            && ref($contract->{loaded_functions}) eq 'HASH'
            && ref($contract->{builtin_overrides}) eq 'HASH'
            && ref($contract->{builtin_override_facts}) eq 'HASH'
            && ref($contract->{loaded_state}) eq 'HASH'
            && ref($contract->{observations}) eq 'HASH'
            && ref($contract->{get16u_probe}) eq 'HASH';
    return $contract;
}

sub fact_identity_matches {
    my ($left, $right) = @_;
    return 0 unless ref($left) eq 'HASH' && ref($right) eq 'HASH';
    for my $key (qw(__perl __opaque __name resolved __deparse source_file source_sha256)) {
        my ($a, $b) = ($left->{$key}, $right->{$key});
        return 0 if defined($a) != defined($b);
        return 0 if defined($a) && $a ne $b;
    }
    return 1;
}

sub has_override {
    my ($overrides) = @_;
    return 1 unless ref($overrides) eq 'HASH';
    return grep { $overrides->{$_} } qw(unpack pack);
}

# Finalise against an already-loaded parent without executing SetByteOrder or
# Get16u again. This is the safe wrapper used after dump_tables.pl has walked
# tables, and is also callable by oracle.pl after its module walk.
sub finalise_loaded_contract {
    my ($contract, $lib_abs) = @_;
    return $contract unless ref($contract) eq 'HASH'
        && ($contract->{kind} // '') eq 'binary_unsigned_reader_contract_v1';
    return $contract if exists($contract->{resolved}) && !$contract->{resolved};
    my $parent_override_facts = builtin_override_facts($lib_abs);
    my $parent_overrides = builtin_overrides($parent_override_facts);
    $contract->{isolated_functions} = $contract->{loaded_functions};
    $contract->{isolated_builtin_overrides} = $contract->{builtin_overrides};
    $contract->{isolated_builtin_override_facts} = $contract->{builtin_override_facts};
    $contract->{isolated_loaded_state} = $contract->{loaded_state};
    $contract->{loaded_state} = loaded_state();
    my $parent_functions = {
        get16u => code_source_fact('Image::ExifTool::Get16u', $lib_abs),
        do_unpack_std => code_source_fact('Image::ExifTool::DoUnpackStd', $lib_abs),
        set_byte_order => code_source_fact('Image::ExifTool::SetByteOrder', $lib_abs),
        get_byte_order => code_source_fact('Image::ExifTool::GetByteOrder', $lib_abs),
    };
    $contract->{loaded_functions} = $parent_functions;
    $contract->{builtin_overrides} = $parent_overrides;
    $contract->{builtin_override_facts} = $parent_override_facts;
    if (has_override($contract->{isolated_builtin_overrides}) || has_override($parent_overrides)) {
        $contract->{resolved} = JSON::PP::false;
        $contract->{reason} = 'builtin_override_present';
        return $contract;
    }
    my $state = $contract->{loaded_state};
    my $expected_state = ref($state) eq 'HASH'
        ? $contract->{observations}{orders}{$state->{reported_byte_order}}
        : undef;
    unless (ref($expected_state) eq 'HASH'
        && defined($state->{unpack_std_s})
        && defined($expected_state->{unpack_std_s})
        && $state->{unpack_std_s} eq $expected_state->{unpack_std_s}) {
        $contract->{resolved} = JSON::PP::false;
        $contract->{reason} = 'parent_loaded_state_mismatch';
        return $contract;
    }
    for my $name (sort keys %$parent_functions) {
        unless (fact_identity_matches($contract->{isolated_functions}{$name}, $parent_functions->{$name})) {
            $contract->{resolved} = JSON::PP::false;
            $contract->{reason} = 'parent_loaded_primitive_mismatch';
            return $contract;
        }
    }
    $contract->{resolved} = JSON::PP::true;
    return $contract;
}

1;
