# Security policy

## Supported versions

This crate is in a `0.x` series and every release is a pre-release. There is no
long-term support branch: a fix ships in a new release cut from `main`, and
older versions are not patched. If you hit a security issue, expect the fix in
the next release rather than a backport.

## What is in scope

`gputrace-bundle` is pure safe Rust: it contains no `unsafe` in the parser, and
links no framework. It reads an untrusted `.gputrace` capture bundle (an `xdic`
index over a zlib store), so the interesting failures are the parser
mishandling malformed or hostile input:

- Parser robustness against malformed or hostile `.gputrace` input: panics,
  unbounded allocation or out-of-memory, integer-overflow-driven misbehavior,
  or incorrect parsing presented to the caller as correct.
- The release and publishing path: the publishing workflow, its
  trusted-publishing configuration, or an archive / tag that does not match the
  source it claims to build from.

## What is not in scope

- Anything that can only be reproduced with a capture you cannot share. Without
  a repro there is nothing to fix; see below for what to send.
- Behavior on inputs that are not `.gputrace` bundles. Feeding the reader
  something else and getting an error or a nonsense result is expected.

## Reporting a vulnerability

Please report privately through GitHub's private vulnerability reporting: open
the repository's **Security** tab and choose **Report a vulnerability**. Do not
open a public issue for a suspected vulnerability.

To let a fix happen quickly, include:

- the smallest `.gputrace` bundle (or raw bytes) that triggers it;
- `rustc -Vv`;
- the crate version involved.

This is a one-person project. Replies are best-effort and usually land within a
few days.
