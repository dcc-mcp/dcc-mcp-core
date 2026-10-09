# Source-only native capture preparation contract

The JSON schemas in this directory are copied without edits from the native
owner's fixed source handoff `native-capture-preparation-protocol-20261009-1`.
The handoff contract SHA256 is
`7ae0f425e391620bb4fd22baf32db3a19c603cbc40a7a6f584dad71b50890221`;
its manifest SHA256 is
`26365478e30867df324f08195ad460d6983b1dfe13e609ddf6ce062cdfd7263b`.

The native base is `edcfdafdef846129d31e0a3c65cbaf9e3736c920` with explicitly
uncommitted protocol changes. The base revision and old `.11` version label do
not identify a runnable candidate. No new binary, tools/list or GUI acceptance
was included. Tests validate actual outgoing public requests against these
source-derived schemas; they do not claim native runtime compatibility.
