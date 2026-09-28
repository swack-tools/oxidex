# Filesystem contract for metadata writes

CLI, library and C FFI writes use the same filesystem transaction. A rewrite
commits a new inode, so the transaction first guards the destination and captures
its supported filesystem metadata. Checking only the directory's ability to
rename, or copying only permission bits, bypasses the source inode's protections.

Before creating scratch files, writes refuse symbolic links (including dangling
links), nonregular files, files with no write permission bits, and files that
cannot be opened for writing under the caller's actual access rights, including
ACLs. Links are never redirected. Setuid/setgid files and Linux files with
`security.capability` are refused to avoid conferring privileges on rewritten
bytes. Linux `security.ima` and `security.evm` attributes also cause refusal:
copying these integrity records would not authenticate the rewritten content.
Darwin immutable and append-only flags are also refused.

Linux and Darwin updates preserve owner, group, mode, and the exact extended
attribute name/value set. Linux POSIX ACLs and security labels are included as
extended attributes. Darwin also preserves extended ACLs and BSD file flags;
resource forks, Finder information and labels are included in its attributes.
Ownership is restored before mode, then ACLs and attributes, with equality
verification before commit. Unsupported metadata, inability to read or restore
it, and verification mismatches cause refusal.

Attribute capture accepts at most 64 KiB of encoded attribute names, 8 MiB per
attribute value, and 16 MiB of attribute values in total. These bounds apply on
Linux and Darwin, including to Darwin resource forks. A value exactly at a bound
is accepted; exceeding any bound refuses the write without truncating or dropping
attributes. The initial capture enforces these limits before scratch or backup
creation, leaving the original file and any existing backup unchanged.

Windows retains the existing permissions-only atomic-write behavior and the
shared symbolic-link, readonly and actual writable-open guards. Permissions are
restored and verified on the replacement before backup/commit. Expanded owner,
group, ACL and extended-attribute preservation in this repair applies to Linux
and macOS. Windows security descriptors (including audit ACLs), alternate data
streams and other native metadata remain inherited limitations of the existing
replacement transaction; this repair does not claim to preserve them. Other
operating systems refuse writes because filesystem metadata preservation is not
implemented.

The writer may replace the scratch inode; the transaction reopens that inode
before restoration and synchronization. Supported metadata restoration and
verification finish before the CLI backup callback. Refused writes leave destination bytes,
its filesystem metadata, symbolic links and their targets, and existing backups
untouched. Scratch files are removed on failure. Successful byte-identical writes
keep the original inode and do not create backups. Updated writes atomically
rename the prepared inode over the destination. Modification/change timestamps
reflect the rewrite; access times can reflect reads. Inode number and hard-link relationships are not preserved.

Callers must exclude concurrent modifications of the destination and its parent
directory. The transaction checks inode identity and security metadata before
commit, but the final path rename is not a compare-and-swap. Backup callback or
rename failures retain the existing transaction's behavior; backup creation is
not rolled back if the subsequent rename fails.
