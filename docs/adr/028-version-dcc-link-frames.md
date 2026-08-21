# Version DCC-Link Frames with a Tagged Header

Status: Accepted

## Context

The original DCC-Link frame was `[len][type][seq][body]`. It could not identify
the body contract used by its writer, so mixed host and sidecar releases could
silently disagree until body decoding failed. Simply inserting a version value
from the existing message-type range would let old readers misinterpret it as a
business message.

## Decision

Version 1 frames use `[len][0x80|version][type][seq][body]`. The high bit is a
format discriminator because all legacy message types are in the `1..=8`
range. Readers accept the original layout as legacy version 0, accept the
current version, and reject every other tagged version with
`UnsupportedDccLinkVersion`.

`DccLinkFrame::new` and the Python constructor emit the current version.
`DccLinkFrame::legacy` and Python `version=0` exist only for rolling upgrades.
A responder should preserve the request version when it must serve both frame
formats.

Rollout order is:

1. Deploy dual-format readers while writers continue using version 0.
2. After every peer can read version 1, switch writers to the default current
   version.
3. Keep legacy decoding for the documented compatibility window; remove legacy
   writing before legacy reading.

## Consequences

- New readers identify the body contract before interpreting the message.
- Old readers reject versioned frames at the type boundary instead of silently
  decoding the wrong fields.
- Rolling deployments have an explicit legacy-write escape hatch.
- Future versions require an intentional compatibility or negotiation change;
  they cannot be accepted accidentally.

## Alternatives considered

- A raw version byte in the `1..=8` range was rejected because old readers
  would treat it as a valid message type.
- Dropping legacy reads immediately was rejected because DCC hosts and sidecars
  cannot always be upgraded atomically.
