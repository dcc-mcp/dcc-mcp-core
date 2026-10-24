use super::*;
use crate::domain::install::InstallStep;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const PAYLOAD: &[u8] = b"verified archive fixture\n";

fn archive_bytes() -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file("payload.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(PAYLOAD).unwrap();
    archive.finish().unwrap().into_inner()
}

fn serve_archive(
    replies: Vec<(&'static str, Option<&'static str>)>,
    archive: &[u8],
) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/archive.zip", listener.local_addr().unwrap());
    let body = archive.to_vec();
    let server = thread::spawn(move || {
        for (index, (status, location)) in replies.into_iter().enumerate() {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "HTTP fixture was not requested");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("HTTP fixture accept failed: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut buffer = [0; 512];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0, "HTTP fixture request ended before its headers");
                request.extend_from_slice(&buffer[..count]);
                assert!(request.len() <= 8192, "HTTP fixture headers are too large");
            }
            let path = if index == 0 {
                "/archive.zip"
            } else {
                "/final.zip"
            };
            assert!(request.starts_with(format!("GET {path} HTTP/1.1\r\n").as_bytes()));
            let location = location
                .map(|value| format!("Location: {value}\r\n"))
                .unwrap_or_default();
            let header = format!(
                "HTTP/1.1 {status}\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let mut response = header.into_bytes();
            response.extend_from_slice(&body);
            stream.write_all(&response).unwrap();
        }
    });
    (url, server)
}

#[test]
fn unsuccessful_archive_http_status_cannot_create_the_destination() {
    let bytes = archive_bytes();
    let checksum = hex::encode(Sha256::digest(&bytes));
    for status in ["404 Not Found", "500 Internal Server Error", "302 Found"] {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("not-created/adapter");
        let (url, server) = serve_archive(vec![(status, None)], &bytes);
        let result = execute_zip_extract(&url, Some(&checksum), &dest);
        server.join().unwrap();
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            InstallError::StepFailed { ref step, .. } if step == "zip-download"
        ));
        assert!(!dest.parent().unwrap().exists());
    }
}

#[test]
fn archive_status_gate_checks_the_final_response_without_disabling_redirects() {
    let bytes = archive_bytes();
    let checksum = hex::encode(Sha256::digest(&bytes));
    for (redirect, status) in [
        (false, "200 OK"),
        (true, "200 OK"),
        (true, "404 Not Found"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("not-created/adapter");
        let mut replies = Vec::new();
        if redirect {
            replies.push(("302 Found", Some("/final.zip")));
        }
        replies.push((status, None));
        let (url, server) = serve_archive(replies, &bytes);
        let result = execute_zip_extract(&url, Some(&checksum), &dest);
        server.join().unwrap();
        if status == "200 OK" {
            let rollback = result.unwrap().unwrap();
            assert_eq!(std::fs::read(dest.join("payload.txt")).unwrap(), PAYLOAD);
            verify_installed_path(&dest).unwrap();
            execute_rollback(&rollback).unwrap();
            assert!(!dest.exists());
        } else {
            assert!(matches!(
                result,
                Err(InstallError::StepFailed { ref step, .. }) if step == "zip-download"
            ));
            assert!(!dest.parent().unwrap().exists());
        }
    }
}

#[test]
fn archive_http_failure_preserves_the_acquire_report_contract() {
    let root = tempfile::tempdir().unwrap();
    let dest = root.path().join("not-created/adapter");
    let bytes = archive_bytes();
    let checksum = hex::encode(Sha256::digest(&bytes));
    let (url, server) = serve_archive(vec![("404 Not Found", None)], &bytes);
    let plan = InstallPlan {
        dcc_type: "godot".into(),
        version: None,
        dcc_path: None,
        adapter: serde_json::from_value(serde_json::json!({
            "name": "archive-fixture",
            "description": "Local HTTP archive fixture"
        }))
        .unwrap(),
        catalog: None,
        steps: vec![InstallStep {
            name: "Acquire archive".into(),
            description: "Use the real HTTP acquisition action".into(),
            action: Some(InstallStepAction::ZipExtract {
                url,
                sha256: Some(checksum),
                dest: dest.clone(),
            }),
        }],
        next_steps: vec![],
        install_policy: InstallPolicy::enabled(),
    };
    let report = InstallService::bundled().execute_plan(&plan, true);
    server.join().unwrap();
    assert_eq!(report.stage, "acquire");
    assert_eq!(report.exit_code, 20);
    assert_eq!(report.error.unwrap().code, "ACQUIRE_STEP_FAILED");
    assert_eq!(report.steps[0].status, "failed");
    assert!(!report.rollback.attempted);
    assert!(report.receipt_path.is_none());
    assert!(!report.verify.directly_usable);
    assert!(!dest.parent().unwrap().exists());
}
