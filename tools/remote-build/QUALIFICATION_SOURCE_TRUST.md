# Qualification source admission contract

The controller has two explicit modes. `maintainer-ssh` retains the exact
maintainer author, committer, SSH signature and principal check. For an actual
GitHub squash integration commit, `github-squash-attested` requires a detached
maintainer SSH signature over `document.json` in namespace
`oxidex-qualification-source-v1`. Git's `E` signature status alone never
authorizes this mode.

The issuer is `qualification_source.py issue`. It fetches the current GitHub
commit, merged PR and integration ref itself, checks their exact commit and
tree, compares GitHub's signature and signed payload to the local raw commit,
and verifies that raw commit's PGP signature with an isolated `GNUPGHOME` and
the full approved GitHub fingerprint. It writes public evidence and an
**unsigned** document. The maintainer controller reviews that evidence and
signs the exact document bytes with the existing approved SSH key. The
controller must prepare the exact Git bundle and run ID before issuing; those
bytes are in the document. Pass the signed directory to `qualification.py`
with `--source-attestation`. The signed directory also contains the prepared
`repository.bundle`.

The Spot launcher must supply a root-owned, read-only public-key anchor at
`/etc/oxidex-remote-build/qualification-maintainer.allowed_signers` inside
the qualification container. It must contain the approved
`swackhamer@users.noreply.github.com` Ed25519 public key with fingerprint
`SHA256:187iTiUNnGG/OCFfAG+4wy37COW7tYTHusirU8CfC4Q`. This anchor is
independent of `/src/maintainer.allowed_signers`, which came from the uploader.
The launcher must implement `verify-source --head <full-sha>
--bundle-sha256 <sha256> --run-id <id>` using host-owned code and that key,
before allowing `/src/qualification_bootstrap.py` or other uploaded source
code to execute. It must verify the document signature, namespace, principal,
schema, raw commit, tree, both pins, evidence digests, exact bundle and run ID.
The launcher must refuse direct uploaded-code execution for an attested run
unless that exact verified source remains bound to the namespace. Until the
anchor and launcher gate are provisioned and reviewed, the attested path
fails closed at `verify-source` and no qualification result may be published.

The uploaded Python repeats identity verification after cloning and at
input, result-pack and replay stages. Those checks defend against drift; they
do not replace the host-owned pre-execution gate. The Spot-generated
`spot-qualification@local` key is only for measurement checkouts and never
authorizes an incoming source. Receipts label the attested authority as a
remotely verified maintainer attestation of the controller's PGP and GitHub
proof, not as remote PGP verification.
