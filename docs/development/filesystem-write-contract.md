# Filesystem contract for metadata writes

CLI, library and C FFI writes use the same filesystem transaction. A rewrite
commits a new inode, so the transaction first guards the destination and captures
its security metadata. Checking only the directory's ability to rename, or
copying only permission bits, bypasses the source inode's protections.

Before creating scratch files, writes refuse symbolic links (including dangling
links), nonregular files, files with no write permission bits, and files that
cannot be opened for writing under the caller's actual access rights, including
ACLs. Links are never redirected. Setuid/setgid files and Linux files with
`security.capability` are refused to avoid conferring privileges on rewritten
bytes. Darwin immutable and append-only flags are also refused.

Linux and Darwin updates preserve owner, group, mode, and the exact extended
attribute name/value set. Linux POSIX ACLs and security labels are included as
extended attributes. Darwin also preserves extended ACLs and BSD file flags;
resource forks, Finder information and labels are included in its attributes.
Ownership is restored before mode, then ACLs and attributes, with equality
verification before commit. Unsupported metadata, inability to read or restore
it, and verification mismatches cause refusal. Other operating systems currently
refuse writes because filesystem metadata preservation is not implemented.

The writer may replace the scratch inode; the transaction reopens that inode
before restoration and synchronization. Metadata restoration and verification
finish before the CLI backup callback. Refused writes leave destination bytes,
its inode security metadata, symbolic links and their targets, and existing backups
untouched. Scratch files are removed on failure. Successful byte-identical writes
keep the original inode and do not create backups. Updated writes atomically
rename the prepared inode over the destination. Modification/change timestamps
reflect the rewrite; access times can reflect reads. Inode number and hard-link relationships are not preserved.

Callers must exclude concurrent modifications of the destination and its parent
directory. The transaction checks inode identity and security metadata before
commit, but the final path rename is not a compare-and-swap. Backup callback or
rename failures retain the existing transaction's behavior; backup creation is
not rolled back if the subsequent rename fails.
