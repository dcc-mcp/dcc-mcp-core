//! Resolution of the process that holds a listening TCP port (#2405).
//!
//! This is the recovery side of port ownership: a gateway can stay alive and
//! keep accepting TCP connections after its embedded service stops answering
//! HTTP, in which case binding the port can never succeed and the only bounded
//! recovery is to terminate the stale holder. Terminating a process is
//! destructive, so this module only ever reports evidence — it never decides
//! anything.
//!
//! Resolution is best-effort. An empty result means "no evidence", never
//! "nobody holds the port": callers must not read it as permission to kill
//! anything, only as a reason to keep waiting.
//!
//! Kept in its own module rather than in `lib.rs` to stay under the
//! repository's 1500-line Rust file-size gate (issue #842). The split is by
//! responsibility — one platform port table in, owning PIDs out — with the
//! tests moved along with the code they cover.

use std::process::{Command, Stdio};

/// Process IDs that currently hold `port` for a TCP listener.
///
/// This is the recovery-side counterpart to [`crate::stop_process`]: a gateway can
/// stay alive and keep accepting TCP connections after its embedded service
/// stops answering HTTP, in which case binding the port can never succeed and
/// the only bounded recovery is to terminate the stale holder (issue #2405).
///
/// Every PID the port table attributes to the listener is reported, so a
/// socket shared through `SO_REUSEPORT` or by a pre-forking server (`nginx`,
/// `httpd`) yields several PIDs rather than one arbitrary member of the
/// group. Callers that need an unambiguous owner must check the length of the
/// result and keep waiting when it is not exactly one.
///
/// Resolution is best-effort. An empty result means "no evidence", never
/// "nobody holds the port" — callers must not treat it as permission to kill
/// anything, only as a reason to keep waiting.
pub fn listener_pids_on_port(port: u16) -> Vec<u32> {
    // Row listings carry the owner directly: `netstat -ano` has a PID column
    // and `ss -ltnp` annotates `users:(("name",pid=N,fd=F))`.
    let pids = parse_listener_pids(&port_holder_table(), port);
    if !pids.is_empty() {
        return pids;
    }
    // Nothing resolved. Retry through `lsof` in field mode, which reports the
    // PID as its own field instead of a whitespace column — the only
    // unambiguous way to read lsof, and the only probe that works on macOS,
    // which ships no `ss`.
    parse_lsof_field_pids(&lsof_field_table(), port)
}

/// Port-table probes for this platform, in the order they are tried.
///
/// Every entry must produce rows [`parse_listener_pids`] can read. On Unix
/// that is `ss` alone: an `lsof` row ends in a `(LISTEN)` state token and its
/// PID column is ambiguous, so lsof rows are ignored by design (see
/// `lsof_rows_are_ignored_because_their_columns_are_ambiguous`) and lsof is
/// only ever read in field mode. Probing lsof as a *row* source made every
/// lookup on an image that ships both tools return "unknown" — which is every
/// Linux CI image — and a service-dead port holder could never be identified
/// (issue #2405).
fn port_holder_probes() -> &'static [(&'static str, &'static [&'static str])] {
    #[cfg(windows)]
    {
        // `netstat -ano` prints one row per socket with the owning PID in the
        // last column. Fixed English keywords are used, and the numeric
        // columns are parsed positionally, so localized status text cannot
        // break PID extraction.
        &[("netstat", &["-ano"])]
    }
    #[cfg(unix)]
    {
        // `ss` (iproute2) does not exist on macOS; that platform resolves
        // holders through [`lsof_field_table`] instead.
        &[("ss", &["-ltnp"])]
    }
}

/// `lsof` output in field mode, or empty when `lsof` is unavailable.
///
/// Field mode (`-F`) prints one record per process: a `p<pid>` line followed
/// by an `n<address>` line per socket. The PID is therefore a dedicated field
/// rather than an ambiguous whitespace column, which is what makes lsof usable
/// at all as a holder source. Windows resolves owners through `netstat`, so
/// this probe is Unix-only.
fn lsof_field_table() -> String {
    #[cfg(windows)]
    {
        String::new()
    }
    #[cfg(unix)]
    {
        run_port_holder_probe("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"]).unwrap_or_default()
    }
}

/// Run one port-table probe, returning its output when it produced any.
fn run_port_holder_probe(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    hidden_console(&mut cmd);
    let out = cmd.stdin(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let table = String::from_utf8_lossy(&out.stdout).into_owned();
    (!table.trim().is_empty()).then_some(table)
}

fn port_holder_table() -> String {
    for (program, args) in port_holder_probes() {
        if let Some(table) = run_port_holder_probe(program, args) {
            return table;
        }
    }
    String::new()
}

fn hidden_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        let _ = cmd;
    }
}

/// Extract listening PIDs for `port` from a port-table listing.
///
/// Only rows whose local address ends in `:port` count, so `9765` never
/// matches `19765` or `59765`. Non-listening sockets are skipped even when
/// they are bound to the port — an outbound connection pinned to our local
/// port does not hold the listener.
fn parse_listener_pids(table: &str, port: u16) -> Vec<u32> {
    let mut pids = Vec::new();
    for row in table.lines() {
        let trimmed = row.trim();
        if trimmed.is_empty() || !row_is_listening(trimmed) {
            continue;
        }
        if !row_local_address_matches_port(trimmed, port) {
            continue;
        }
        for pid in row_owning_pids(trimmed) {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

/// True for rows describing a listening TCP socket.
///
/// Windows marks the state explicitly. Unix rows from `ss` start with `LISTEN`;
/// rows from other tools carry `(LISTEN)`. A row with no recognizable state is
/// rejected — under-reporting a holder is safe (the caller waits), while
/// over-reporting authorizes terminating an unrelated process.
fn row_is_listening(row: &str) -> bool {
    if row.contains("LISTENING") || row.starts_with("LISTEN") || row.contains("(LISTEN)") {
        return true;
    }
    false
}

/// True when the row's *local* address column listens on `port`.
///
/// A Windows netstat row is
/// `<proto> <local> <foreign> <state> <pid>`; the first column is the
/// `TCP`/`UDP` protocol of each data row, while the header row starts with
/// `Proto`. An `ss` row ends in `... <local> <peer> users:(...)`, so its local
/// address is three columns from the end (or two when unannotated). Selecting
/// by row shape rather than one fixed index keeps both layouts correct.
fn row_local_address_matches_port(row: &str, port: u16) -> bool {
    let suffix = format!(":{port}");
    let columns: Vec<&str> = row.split_whitespace().collect();
    let is_netstat_data_row = columns.first().is_some_and(|first| {
        first.eq_ignore_ascii_case("TCP") || first.eq_ignore_ascii_case("UDP")
    });
    if is_netstat_data_row
        && columns
            .get(1)
            .is_some_and(|local| address_column_has_port(local, &suffix))
    {
        return true;
    }
    // `ss`: the local address precedes the peer address.
    let local_index = columns
        .len()
        .checked_sub(3)
        .or_else(|| columns.len().checked_sub(2));
    local_index.is_some_and(|index| address_column_has_port(columns[index], &suffix))
}

fn address_column_has_port(column: &str, suffix: &str) -> bool {
    match column.rfind(suffix) {
        Some(index) => {
            index + suffix.len() == column.len()
                || matches!(column.as_bytes()[index + suffix.len()], b',' | b']' | b')')
        }
        None => false,
    }
}

/// Every PID one port-table row attributes to its socket, in document order.
///
/// A listener can legitimately have several owners: `SO_REUSEPORT` and
/// pre-forking servers (`nginx`, `httpd`) share one listening socket across a
/// worker group, and `ss -ltnp` then annotates the row with one tuple per
/// member — `users:(("httpd",pid=2355,fd=4),("httpd",pid=1962,fd=4))`.
/// Reporting a single member of that group would make the group look like a
/// single unambiguous owner to callers that check `len() == 1`, so the whole
/// group is returned instead and the caller keeps waiting (issue #2488).
fn row_owning_pids(row: &str) -> Vec<u32> {
    // `ss -ltnp` only annotates `users:(...)` for sockets owned by the
    // current user; its last whitespace column is the peer address, so the
    // user annotation is the authoritative source there.
    let pids = pids_from_users_annotation(row);
    if !pids.is_empty() {
        return pids;
    }
    if row.contains("users:(") {
        // Annotated but no PID is readable — do not fall back to a positional
        // guess.
        return Vec::new();
    }
    row.split_whitespace()
        .next_back()
        .and_then(|column| column.parse::<u32>().ok())
        .filter(|pid| *pid > 0)
        .into_iter()
        .collect()
}

/// Extract listening PIDs for `port` from `lsof -Fpn` output.
///
/// Captured verbatim from lsof 4.98 (`-nP -iTCP -sTCP:LISTEN -Fpn`), with two
/// Python listeners and a root-owned wildcard neighbour:
///
/// ```text
/// p360431
/// n127.0.0.1:45997
/// p360433
/// n127.0.0.1:45996
/// ```
///
/// A `p` line opens a process record and every `n` line that follows belongs
/// to it, so the PID never has to be guessed from a column position — unlike
/// lsof's tabular output, which is what makes this the only readable lsof
/// shape. Records are delimited by the next `p` line, not by blank lines, so
/// no separator handling is needed. The port is matched with
/// [`address_column_has_port`], so `:9765` still never matches `:19765`.
///
/// A process listening on the port through several sockets (IPv4 and IPv6, or
/// several file descriptors) reports one `n` line per socket; the PID is
/// collected once.
fn parse_lsof_field_pids(table: &str, port: u16) -> Vec<u32> {
    let suffix = format!(":{port}");
    let mut pids = Vec::new();
    let mut current_pid: Option<u32> = None;

    for line in table.lines() {
        let line = line.trim();
        match line.as_bytes().first() {
            Some(b'p') => {
                current_pid = line[1..].parse::<u32>().ok().filter(|pid| *pid > 0);
            }
            Some(b'n') => {
                let Some(pid) = current_pid else { continue };
                if address_column_has_port(&line[1..], &suffix) && !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
            _ => {}
        }
    }
    pids
}

/// Every distinct PID in an `ss` `users:(...)` annotation, in document order.
///
/// An annotation is one `("name",pid=N,fd=F)` tuple per process that holds a
/// reference to the socket, so the whole row is scanned rather than just its
/// last `pid=` field. Duplicates collapse — the same PID can appear more than
/// once through several file descriptors, and a repeated entry must not make
/// one process look like several holders.
///
/// A `pid=` that is not followed by a positive number is skipped instead of
/// aborting the scan, so one malformed tuple does not hide a valid owner
/// earlier in the row.
fn pids_from_users_annotation(row: &str) -> Vec<u32> {
    const MARKER: &str = "pid=";
    let mut pids = Vec::new();
    let mut rest = row;
    while let Some(offset) = rest.find(MARKER) {
        let digits_start = offset + MARKER.len();
        let digits: String = rest[digits_start..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        if let Some(pid) = digits.parse::<u32>().ok().filter(|pid| *pid > 0)
            && !pids.contains(&pid)
        {
            pids.push(pid);
        }
        // `digits_start` is always past `offset`, so the scan advances past
        // the marker even when the field carries no digits — otherwise a
        // bare `pid=` would loop forever.
        rest = &rest[digits_start + digits.len()..];
    }
    pids
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Port holder resolution tests ───────────────────────────────────

    #[test]
    fn windows_netstat_rows_resolve_listening_pid() {
        let table = "Active Connections\r\n\
\r\n  Proto  Local Address          Foreign Address        State           PID\r\n\
  TCP    0.0.0.0:9765           0.0.0.0:0              LISTENING       4242\r\n\
  TCP    127.0.0.1:19765        0.0.0.0:0              LISTENING       5150\r\n\
  TCP    127.0.0.1:59765        0.0.0.0:0              LISTENING       5151\r\n\
  TCP    [::]:9765              [::]:0                 LISTENING       4242\r\n\
  TCP    10.0.0.1:59765         10.0.0.2:443           ESTABLISHED     909\r\n\
  TCP    10.0.0.1:9765          10.0.0.2:443           ESTABLISHED     910\r\n\
  TCP    10.0.0.1:35847         10.0.0.2:443           ESTABLISHED     911\r\n";

        assert_eq!(parse_listener_pids(table, 9765), vec![4242]);
        assert_eq!(parse_listener_pids(table, 19765), vec![5150]);
        assert_eq!(parse_listener_pids(table, 59765), vec![5151]);
        assert!(
            parse_listener_pids(table, 35847).is_empty(),
            "an ESTABLISHED row is not a listener"
        );
        assert!(
            !parse_listener_pids(table, 9765).contains(&910),
            "a pinned outbound socket on our port must not be reported as the holder"
        );
        assert!(parse_listener_pids(table, 9766).is_empty());
    }

    #[test]
    fn ss_rows_resolve_listening_pid_from_users_annotation() {
        let table = "State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process\n\
LISTEN 0      128    0.0.0.0:9765         0.0.0.0:*         users:((\"dcc-mcp-server\",pid=777,fd=9))\n\
LISTEN 0      128    [::]:19765           [::]:*            users:((\"dcc-mcp-server\",pid=778,fd=10))\n\
LISTEN 0      128    0.0.0.0:59765        0.0.0.0:*         users:((\"dcc-mcp-server\",pid=779,fd=11))\n";

        assert_eq!(parse_listener_pids(table, 9765), vec![777]);
        assert_eq!(parse_listener_pids(table, 19765), vec![778]);
        assert_eq!(parse_listener_pids(table, 59765), vec![779]);
        assert!(parse_listener_pids(table, 9766).is_empty());
    }

    /// Regression test for #2488: `ss -ltnp` annotates a listener shared by a
    /// process group (`SO_REUSEPORT`, or pre-forking servers such as
    /// `httpd`/`nginx`) with one `pid=` tuple per member. Returning only the
    /// last of them made the group look like a single unambiguous owner, so a
    /// caller checking `len() == 1` believed it had identified the holder.
    #[test]
    fn shared_listener_row_reports_every_pid_in_the_users_annotation() {
        // Captured verbatim from `ss -ltnp` for a pre-forking server: two
        // workers share one listening socket.
        let table = "State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process\n\
LISTEN 0      128    0.0.0.0:9765         0.0.0.0:*         users:((\"httpd\",pid=2355,fd=4),(\"httpd\",pid=1962,fd=4))\n";

        let pids = parse_listener_pids(table, 9765);
        assert_eq!(
            pids.len(),
            2,
            "both workers of a shared listener must be reported, not just the last"
        );
        assert!(pids.contains(&2355) && pids.contains(&1962), "got {pids:?}");
        assert_ne!(
            pids.len(),
            1,
            "a shared listener must never satisfy the single-owner invariant"
        );
    }

    /// The caller side of the #2488 invariant: the reap decision requires
    /// exactly one holder, so a shared listener authorizes nothing.
    #[test]
    fn multi_pid_listener_is_never_an_authorized_kill() {
        let table = "State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process\n\
LISTEN 0      128    0.0.0.0:9765         0.0.0.0:*         users:((\"a\",pid=10,fd=3),(\"b\",pid=11,fd=3))\n";

        let pids = parse_listener_pids(table, 9765);
        assert_eq!(pids, vec![10, 11]);
        // Mirrors the guard in `service_dead_port_holder_pid`: anything other
        // than exactly one PID means "ambiguous, keep waiting".
        assert!(
            pids.len() != 1,
            "two PIDs must not collapse into one authorized kill"
        );
        assert!(
            pids.iter().all(|pid| *pid > 0),
            "a zero PID is not evidence of ownership"
        );
    }

    #[test]
    fn repeated_pid_in_one_annotation_collapses_to_one_holder() {
        // One process holding the socket through several file descriptors is
        // still one holder; a duplicate entry must not look like a second
        // process.
        let table = "State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process\n\
LISTEN 0      128    0.0.0.0:9765         0.0.0.0:*         users:((\"a\",pid=10,fd=3),(\"a\",pid=10,fd=7))\n";

        assert_eq!(parse_listener_pids(table, 9765), vec![10]);
    }

    #[test]
    fn malformed_pid_field_does_not_hide_an_earlier_owner() {
        // A truncated `pid=` at the end of the row must not discard the owner
        // parsed before it, and must not fall back to a positional guess
        // either (the row is annotated).
        let table = "State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process\n\
LISTEN 0      128    0.0.0.0:9765         0.0.0.0:*         users:((\"a\",pid=10,fd=3),(\"b\",pid=,fd=4))\n";

        assert_eq!(parse_listener_pids(table, 9765), vec![10]);
    }

    #[test]
    fn users_annotation_without_any_pid_yields_no_holder() {
        let table = "State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process\n\
LISTEN 0      128    0.0.0.0:9765         0.0.0.0:*         users:((\"a\",fd=3))\n";

        assert!(
            parse_listener_pids(table, 9765).is_empty(),
            "an annotated row with no readable PID is unknown, never a guessed owner"
        );
    }

    #[test]
    fn real_ss_output_shape_resolves_only_the_annotated_listener() {
        // Captured verbatim from `ss -ltnp` on Linux (iproute2) as an
        // unprivileged user: only sockets owned by the caller carry a
        // `users:(...)` annotation. Every unannotated neighbour must stay
        // "unknown" rather than resolve to a guessed PID.
        let table = "State  Recv-Q Send-Q  Local Address:Port  Peer Address:PortProcess\n\
LISTEN 0      1000   10.255.255.254:53         0.0.0.0:*\n\
LISTEN 0      5           127.0.0.1:45999      0.0.0.0:*    users:((\"python3\",pid=312260,fd=3))\n\
LISTEN 0      128           0.0.0.0:53559      0.0.0.0:*\n\
LISTEN 0      128              [::]:53559         [::]:*\n";

        assert_eq!(parse_listener_pids(table, 45999), vec![312_260]);
        assert!(
            parse_listener_pids(table, 53).is_empty(),
            "an unannotated row is unknown, never a guessed PID"
        );
        assert!(parse_listener_pids(table, 53559).is_empty());
    }

    #[test]
    fn ss_rows_without_user_annotation_yield_no_pid() {
        // Without `users:(...)` the trailing column is the peer address —
        // parsing it as a PID would attribute the port to an unrelated
        // process. Better to report "unknown" than a wrong owner.
        let table = "State Recv-Q Send-Q Local Address:Port Peer Address:Port\n\
LISTEN 0      128    127.0.0.1:9765       0.0.0.0:*\n";

        assert!(parse_listener_pids(table, 9765).is_empty());
    }

    #[test]
    fn lsof_rows_are_ignored_because_their_columns_are_ambiguous() {
        // lsof ends in a `(LISTEN)` state token, not a PID. The shared parser
        // applies the `ss` user-annotation rule on Unix, so an lsof row
        // yields no PID rather than a wrong one.
        let table = "COMMAND     PID   USER   FD   TYPE  DEVICE SIZE/OFF NODE NAME\n\
dcc-mcp-se 4242 hallong   12u  IPv4 0x1234      0t0  TCP 127.0.0.1:9765 (LISTEN)\n";

        assert!(parse_listener_pids(table, 9765).is_empty());
    }

    #[test]
    fn real_lsof_field_output_resolves_the_owning_pid() {
        // Captured verbatim from lsof 4.98 with
        // `-nP -iTCP -sTCP:LISTEN -Fpn`. One process listening on both an
        // IPv4 and an IPv6 socket: both addresses must resolve to it, once.
        let table = "p360282\nn127.0.0.1:45999\nn[::1]:45998\n";

        assert_eq!(parse_lsof_field_pids(table, 45999), vec![360_282]);
        assert_eq!(
            parse_lsof_field_pids(table, 45998),
            vec![360_282],
            "a second socket of the same listener is the same holder"
        );
        assert!(parse_lsof_field_pids(table, 45997).is_empty());
    }

    #[test]
    fn real_lsof_field_output_separates_processes_and_dedupes_sockets() {
        // Captured verbatim through `sudo` (lsof 4.98, field mode), so the
        // root-owned wildcard listener is visible too: one `p` record per
        // process, one `n` line per socket, no blank separators.
        let table =
            "p503\nn*:53559\nn*:53559\np360431\nn127.0.0.1:45997\np360433\nn127.0.0.1:45996\n";

        assert_eq!(
            parse_lsof_field_pids(table, 53559),
            vec![503],
            "two sockets on one port are one holder, not two"
        );
        assert_eq!(parse_lsof_field_pids(table, 45997), vec![360_431]);
        assert_eq!(parse_lsof_field_pids(table, 45996), vec![360_433]);
        assert!(parse_lsof_field_pids(table, 9766).is_empty());
    }

    #[test]
    fn lsof_field_output_never_matches_a_port_suffix() {
        let table = "p5151\nn127.0.0.1:59765\n";

        assert!(parse_lsof_field_pids(table, 9765).is_empty());
        assert!(parse_lsof_field_pids(table, 976).is_empty());
        assert_eq!(parse_lsof_field_pids(table, 59765), vec![5151]);
    }

    #[test]
    fn lsof_field_output_without_a_pid_record_yields_no_pid() {
        // An `n` line before any `p` line has no owner to attribute the port
        // to. Reporting "unknown" keeps the caller waiting instead of killing
        // a guessed process.
        assert!(parse_lsof_field_pids("n127.0.0.1:9765\n", 9765).is_empty());
        assert!(parse_lsof_field_pids("p0\nn127.0.0.1:9765\n", 9765).is_empty());
        assert!(parse_lsof_field_pids("", 9765).is_empty());
    }

    #[test]
    fn port_prefix_sharing_never_matches_across_lengths() {
        // `:9765` must not match a row for `59765`, and `:976` must not match
        // either — the suffix has to be the whole port field.
        let table =
            "  TCP    127.0.0.1:59765        0.0.0.0:0              LISTENING       5151\r\n";

        assert!(parse_listener_pids(table, 9765).is_empty());
        assert!(parse_listener_pids(table, 976).is_empty());
        assert_eq!(parse_listener_pids(table, 59765), vec![5151]);
    }

    #[test]
    fn table_header_and_non_tcp_rows_are_ignored() {
        let table = "  Proto  Local Address          Foreign Address        State           PID\r\n\
  UDP    0.0.0.0:9765           0.0.0.0:*                              4242\r\n\
Active Connections\r\n";

        assert!(
            parse_listener_pids(table, 9765).is_empty(),
            "the column header and UDP rows must not resolve to a PID"
        );
    }

    // ── Probe ordering (#2405) ─────────────────────────────────────────

    /// Regression test for #2405: `lsof` used to be probed before `ss`, and
    /// even after the reorder it contributed nothing — the shared row parser
    /// can only read `ss`-style `users:(...)` annotations, so an `lsof` row
    /// always resolved to "unknown" and a service-dead holder could never be
    /// identified on an image that ships both tools (every Linux CI image).
    /// lsof is now read separately, in field mode.
    #[cfg(unix)]
    #[test]
    fn unix_row_probes_never_use_lsof_rows() {
        let programs: Vec<&str> = port_holder_probes().iter().map(|(p, _)| *p).collect();
        assert_eq!(
            programs,
            vec!["ss"],
            "only `ss` rows carry a parseable PID; lsof is read in field mode"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_probes_netstat() {
        let programs: Vec<&str> = port_holder_probes().iter().map(|(p, _)| *p).collect();
        assert_eq!(programs, vec!["netstat"]);
    }

    /// End-to-end regression test for #2405 on the platform that runs the
    /// Rust suite: a listener owned by this process must be resolvable.
    ///
    /// This is the check the port-recovery tests depend on — they spawn a
    /// real holder child and poll until it can be resolved. Whichever probe
    /// answers (`ss`, or `lsof` in field mode) has to name *this* process,
    /// and an image with none of them skips instead of failing.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_resolves_our_own_listener_pid() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let pids = listener_pids_on_port(port);

        if pids.is_empty() {
            eprintln!("skipping: no port-table tool on this image resolves a listener owner");
            return;
        }
        assert_eq!(
            pids,
            vec![std::process::id()],
            "a listener owned by this process must resolve to this process"
        );
        drop(listener);
    }
}
