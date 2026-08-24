use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::domain::feedback_file::{
    FEEDBACK_FILE_SCHEMA_VERSION, FeedbackFileError, FeedbackFilingRecommendation,
    FeedbackIssueCandidate, build_issue_document, recommend_filing,
};

use super::feedback::{FeedbackRouteService, FeedbackRouteSnapshot};

const DEFAULT_CATALOG_PATH: &str = "dcc-mcp-catalog.yml";
pub(crate) const BUNDLED_CATALOG_SENTINEL: &str = "@bundled:dcc-mcp-catalog:v1";
const MAX_EXACT_RESULTS: usize = 20;
const MAX_KEYWORD_RESULTS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackFileDecision {
    Existing(u64),
    Create,
}

#[derive(Debug, Clone)]
pub struct FeedbackFileRequest {
    pub finding_path: PathBuf,
    pub catalog_path: Option<PathBuf>,
    pub decision: Option<FeedbackFileDecision>,
    pub authorized: bool,
    pub authorization: Option<FeedbackFileAuthorization>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackFileAuthorization {
    pub canonical_finding_path: PathBuf,
    pub finding_sha256: String,
    pub fingerprint: String,
    pub repo: String,
    pub catalog_identity: FeedbackCatalogIdentity,
    pub catalog_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackCatalogIdentity {
    Bundled,
    CanonicalPath(PathBuf),
}

impl FeedbackCatalogIdentity {
    pub(crate) fn from_plan_value(value: impl Into<String>) -> Self {
        let value = value.into();
        if value == BUNDLED_CATALOG_SENTINEL {
            Self::Bundled
        } else {
            Self::CanonicalPath(PathBuf::from(value))
        }
    }

    fn plan_value(&self) -> Result<String, FeedbackFileServiceError> {
        match self {
            Self::Bundled => Ok(BUNDLED_CATALOG_SENTINEL.to_string()),
            Self::CanonicalPath(path) => path
                .to_str()
                .map(str::to_string)
                .ok_or(FeedbackFileServiceError::NonUnicodePath),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackIssueSearchField {
    Title,
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackIssueState {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackIssueRecord {
    pub candidate: FeedbackIssueCandidate,
    pub state: FeedbackIssueState,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct FeedbackIssueTrackerError {
    pub message: String,
}

pub trait FeedbackIssueTracker {
    fn search_open(
        &self,
        repo: &str,
        query: &str,
        fields: &[FeedbackIssueSearchField],
        limit: usize,
    ) -> Result<Vec<FeedbackIssueRecord>, FeedbackIssueTrackerError>;

    fn view_issue(
        &self,
        repo: &str,
        number: u64,
    ) -> Result<FeedbackIssueRecord, FeedbackIssueTrackerError>;

    fn comment_issue(
        &self,
        repo: &str,
        number: u64,
        body: &str,
    ) -> Result<(), FeedbackIssueTrackerError>;

    fn create_issue(
        &self,
        repo: &str,
        title: &str,
        body: &str,
    ) -> Result<FeedbackIssueCandidate, FeedbackIssueTrackerError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeedbackFileNextStep {
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeedbackFileDedup {
    pub exact: Vec<FeedbackIssueCandidate>,
    pub keyword: Vec<FeedbackIssueCandidate>,
    pub keyword_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackFileStatus {
    Planned,
    Commented,
    Created,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeedbackFileResult {
    pub schema_version: &'static str,
    pub authorized: bool,
    pub status: FeedbackFileStatus,
    pub repo: String,
    pub fingerprint: String,
    pub recommendation: FeedbackFilingRecommendation,
    pub dedup: FeedbackFileDedup,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<FeedbackFileNextStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_options: Vec<FeedbackFileNextStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<FeedbackIssueCandidate>,
}

#[derive(Debug, Error)]
pub enum FeedbackFileServiceError {
    #[error("--yes requires exactly one filing decision: --existing <number> or --create")]
    AuthorizationDecisionRequired,
    #[error("--yes requires the complete authorization binding emitted by a prior plan")]
    AuthorizationBindingRequired,
    #[error("authorization no longer matches the reviewed feedback plan")]
    AuthorizationMismatch,
    #[error(transparent)]
    Finding(#[from] crate::application::feedback::FeedbackRouteServiceError),
    #[error(transparent)]
    Document(#[from] FeedbackFileError),
    #[error(transparent)]
    Tracker(#[from] FeedbackIssueTrackerError),
    #[error("finding or catalog path cannot be represented in an executable argv")]
    NonUnicodePath,
    #[error("fingerprint search reached its bounded result limit")]
    ExactSearchLimitReached,
    #[error("an exact fingerprint match already exists: {issue_numbers:?}")]
    ExactIssueConflict { issue_numbers: Vec<u64> },
    #[error("issue #{number} is not open")]
    IssueNotOpen { number: u64 },
}

pub struct FeedbackFileService<'a, T: FeedbackIssueTracker> {
    tracker: &'a T,
}

impl<'a, T: FeedbackIssueTracker> FeedbackFileService<'a, T> {
    pub fn new(tracker: &'a T) -> Self {
        Self { tracker }
    }

    pub fn file(
        &self,
        request: FeedbackFileRequest,
    ) -> Result<FeedbackFileResult, FeedbackFileServiceError> {
        if request.authorized && request.decision.is_none() {
            return Err(FeedbackFileServiceError::AuthorizationDecisionRequired);
        }
        if request.authorized && request.authorization.is_none() {
            return Err(FeedbackFileServiceError::AuthorizationBindingRequired);
        }

        let inputs = FeedbackFileInputs::capture(&request)?;
        if let Some(authorization) = request.authorization.as_ref() {
            inputs.verify_authorization(authorization)?;
        }
        let exact_records = self.search_exact(&inputs.route.repo, &inputs.finding.fingerprint)?;
        let exact = candidates(&exact_records);
        let (keyword, keyword_truncated) = if exact.is_empty() {
            self.search_keywords(&inputs.route.repo, &inputs.document.search_terms)?
        } else {
            (Vec::new(), false)
        };
        let recommendation = recommend_filing(&exact, &keyword);
        let dedup = FeedbackFileDedup {
            exact,
            keyword,
            keyword_truncated,
        };

        if !request.authorized {
            let (next_step, review_options) = plan_next_steps(
                &inputs.authorization,
                request.decision,
                recommendation.clone(),
                &dedup,
            )?;
            return Ok(FeedbackFileResult {
                schema_version: FEEDBACK_FILE_SCHEMA_VERSION,
                authorized: false,
                status: FeedbackFileStatus::Planned,
                repo: inputs.route.repo,
                fingerprint: inputs.finding.fingerprint,
                recommendation,
                dedup,
                next_step,
                review_options,
                issue: None,
            });
        }

        let issue = match request
            .decision
            .expect("authorized requests require a filing decision")
        {
            FeedbackFileDecision::Create => {
                let raced = candidates(
                    &self.search_exact(&inputs.route.repo, &inputs.finding.fingerprint)?,
                );
                if !raced.is_empty() {
                    return Err(exact_conflict(&raced));
                }
                let final_inputs = FeedbackFileInputs::capture(&request)?;
                final_inputs.verify_authorization(
                    request
                        .authorization
                        .as_ref()
                        .expect("authorized requests require an authorization binding"),
                )?;
                self.tracker.create_issue(
                    &final_inputs.route.repo,
                    &final_inputs.document.title,
                    &final_inputs.document.body,
                )?
            }
            FeedbackFileDecision::Existing(number) => {
                let raced = candidates(
                    &self.search_exact(&inputs.route.repo, &inputs.finding.fingerprint)?,
                );
                if !raced.is_empty() && !raced.iter().any(|issue| issue.number == number) {
                    return Err(exact_conflict(&raced));
                }
                let issue = self.tracker.view_issue(&inputs.route.repo, number)?;
                if issue.state != FeedbackIssueState::Open {
                    return Err(FeedbackFileServiceError::IssueNotOpen { number });
                }
                let final_inputs = FeedbackFileInputs::capture(&request)?;
                final_inputs.verify_authorization(
                    request
                        .authorization
                        .as_ref()
                        .expect("authorized requests require an authorization binding"),
                )?;
                self.tracker.comment_issue(
                    &final_inputs.route.repo,
                    number,
                    &final_inputs.document.comment_body,
                )?;
                issue.candidate
            }
        };
        let status = match request.decision {
            Some(FeedbackFileDecision::Create) => FeedbackFileStatus::Created,
            Some(FeedbackFileDecision::Existing(_)) => FeedbackFileStatus::Commented,
            None => unreachable!("authorized requests require a filing decision"),
        };
        Ok(FeedbackFileResult {
            schema_version: FEEDBACK_FILE_SCHEMA_VERSION,
            authorized: true,
            status,
            repo: inputs.route.repo,
            fingerprint: inputs.finding.fingerprint,
            recommendation,
            dedup,
            next_step: None,
            review_options: Vec::new(),
            issue: Some(issue),
        })
    }

    fn search_exact(
        &self,
        repo: &str,
        fingerprint: &str,
    ) -> Result<Vec<FeedbackIssueRecord>, FeedbackFileServiceError> {
        let search_key = fingerprint.strip_prefix("sha256:").unwrap_or(fingerprint);
        let query = format!("\"{search_key}\"");
        let records = self.tracker.search_open(
            repo,
            &query,
            &[
                FeedbackIssueSearchField::Title,
                FeedbackIssueSearchField::Body,
            ],
            MAX_EXACT_RESULTS + 1,
        )?;
        if records.len() > MAX_EXACT_RESULTS {
            return Err(FeedbackFileServiceError::ExactSearchLimitReached);
        }
        Ok(deduplicate_records(
            records
                .into_iter()
                .filter(|record| {
                    record.body.contains(fingerprint)
                        || record.candidate.title.contains(fingerprint)
                })
                .collect(),
        ))
    }

    fn search_keywords(
        &self,
        repo: &str,
        search_terms: &[String],
    ) -> Result<(Vec<FeedbackIssueCandidate>, bool), FeedbackFileServiceError> {
        let query = search_terms
            .iter()
            .map(|term| format!("\"{term}\""))
            .collect::<Vec<_>>()
            .join(" ");
        let records = self.tracker.search_open(
            repo,
            &query,
            &[FeedbackIssueSearchField::Title],
            MAX_KEYWORD_RESULTS + 1,
        )?;
        let truncated = records.len() > MAX_KEYWORD_RESULTS;
        let records = deduplicate_records(records);
        Ok((
            candidates(
                &records
                    .into_iter()
                    .take(MAX_KEYWORD_RESULTS)
                    .collect::<Vec<_>>(),
            ),
            truncated,
        ))
    }
}

struct FeedbackFileInputs {
    finding: dcc_mcp_models::FindingV1,
    route: crate::domain::feedback::FeedbackRoute,
    document: crate::domain::feedback_file::FeedbackIssueDocument,
    authorization: FeedbackFileAuthorization,
}

impl FeedbackFileInputs {
    fn capture(request: &FeedbackFileRequest) -> Result<Self, FeedbackFileServiceError> {
        let snapshot = FeedbackRouteService::new(PathBuf::from(DEFAULT_CATALOG_PATH))
            .snapshot(&request.finding_path, request.catalog_path.as_deref())?;
        Self::from_snapshot(snapshot)
    }

    fn from_snapshot(snapshot: FeedbackRouteSnapshot) -> Result<Self, FeedbackFileServiceError> {
        let document = build_issue_document(&snapshot.finding, &snapshot.route)?;
        let catalog_identity = snapshot.canonical_catalog_path.map_or(
            FeedbackCatalogIdentity::Bundled,
            FeedbackCatalogIdentity::CanonicalPath,
        );
        let authorization = FeedbackFileAuthorization {
            canonical_finding_path: snapshot.canonical_finding_path,
            finding_sha256: sha256_identity(&snapshot.finding_bytes),
            fingerprint: snapshot.finding.fingerprint.clone(),
            repo: snapshot.route.repo.clone(),
            catalog_identity,
            catalog_sha256: sha256_identity(&snapshot.catalog_bytes),
        };
        Ok(Self {
            finding: snapshot.finding,
            route: snapshot.route,
            document,
            authorization,
        })
    }

    fn verify_authorization(
        &self,
        expected: &FeedbackFileAuthorization,
    ) -> Result<(), FeedbackFileServiceError> {
        if &self.authorization == expected {
            Ok(())
        } else {
            Err(FeedbackFileServiceError::AuthorizationMismatch)
        }
    }
}

fn sha256_identity(bytes: &[u8]) -> String {
    let mut identity = "sha256:".to_string();
    for byte in Sha256::digest(bytes) {
        write!(identity, "{byte:02x}").expect("writing to a string cannot fail");
    }
    identity
}

fn candidates(records: &[FeedbackIssueRecord]) -> Vec<FeedbackIssueCandidate> {
    records
        .iter()
        .map(|record| record.candidate.clone())
        .collect()
}

fn deduplicate_records(records: Vec<FeedbackIssueRecord>) -> Vec<FeedbackIssueRecord> {
    records
        .into_iter()
        .map(|record| (record.candidate.number, record))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect()
}

fn exact_conflict(candidates: &[FeedbackIssueCandidate]) -> FeedbackFileServiceError {
    FeedbackFileServiceError::ExactIssueConflict {
        issue_numbers: candidates.iter().map(|issue| issue.number).collect(),
    }
}

fn plan_next_steps(
    authorization: &FeedbackFileAuthorization,
    decision: Option<FeedbackFileDecision>,
    recommendation: FeedbackFilingRecommendation,
    dedup: &FeedbackFileDedup,
) -> Result<(Option<FeedbackFileNextStep>, Vec<FeedbackFileNextStep>), FeedbackFileServiceError> {
    if let Some(decision) = decision {
        return Ok((Some(next_step(authorization, decision)?), Vec::new()));
    }
    match recommendation {
        FeedbackFilingRecommendation::CommentExisting { issue_number } => Ok((
            Some(next_step(
                authorization,
                FeedbackFileDecision::Existing(issue_number),
            )?),
            Vec::new(),
        )),
        FeedbackFilingRecommendation::Create => Ok((
            Some(next_step(authorization, FeedbackFileDecision::Create)?),
            Vec::new(),
        )),
        FeedbackFilingRecommendation::ReviewRequired => {
            let source = if dedup.exact.is_empty() {
                &dedup.keyword
            } else {
                &dedup.exact
            };
            let mut options = source
                .iter()
                .map(|issue| next_step(authorization, FeedbackFileDecision::Existing(issue.number)))
                .collect::<Result<Vec<_>, _>>()?;
            if dedup.exact.is_empty() {
                options.push(next_step(authorization, FeedbackFileDecision::Create)?);
            }
            Ok((None, options))
        }
    }
}

fn next_step(
    authorization: &FeedbackFileAuthorization,
    decision: FeedbackFileDecision,
) -> Result<FeedbackFileNextStep, FeedbackFileServiceError> {
    let finding_path = authorization
        .canonical_finding_path
        .to_str()
        .ok_or(FeedbackFileServiceError::NonUnicodePath)?;
    let mut argv = vec![
        "dcc-mcp-cli".to_string(),
        "feedback".to_string(),
        "file".to_string(),
        finding_path.to_string(),
    ];
    let catalog_identity = authorization.catalog_identity.plan_value()?;
    if let FeedbackCatalogIdentity::CanonicalPath(path) = &authorization.catalog_identity {
        argv.push("--catalog".to_string());
        argv.push(
            path.to_str()
                .ok_or(FeedbackFileServiceError::NonUnicodePath)?
                .to_string(),
        );
    }
    match decision {
        FeedbackFileDecision::Existing(number) => {
            argv.push("--existing".to_string());
            argv.push(number.to_string());
        }
        FeedbackFileDecision::Create => argv.push("--create".to_string()),
    }
    argv.extend([
        "--expected-finding-path".to_string(),
        finding_path.to_string(),
        "--expected-finding-sha256".to_string(),
        authorization.finding_sha256.clone(),
        "--expected-fingerprint".to_string(),
        authorization.fingerprint.clone(),
        "--expected-repo".to_string(),
        authorization.repo.clone(),
        "--expected-catalog-path".to_string(),
        catalog_identity,
        "--expected-catalog-sha256".to_string(),
        authorization.catalog_sha256.clone(),
    ]);
    argv.extend(["--yes".to_string(), "--json".to_string()]);
    Ok(FeedbackFileNextStep { argv })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap, VecDeque};
    use std::process::Command;
    use std::sync::Mutex;

    use dcc_mcp_models::{
        FINDING_V1_SCHEMA_VERSION, FindingEvidenceV1, FindingPhase, FindingRedactionMode,
        FindingRedactionStatusV1, FindingReproV1, FindingSeverity, FindingV1, finding_fingerprint,
    };
    use serde_json::json;

    use super::*;

    #[derive(Default)]
    struct FakeState {
        searches: VecDeque<Vec<FeedbackIssueRecord>>,
        views: HashMap<u64, FeedbackIssueRecord>,
        calls: Vec<String>,
        comments: Vec<u64>,
        creates: usize,
    }

    #[derive(Default)]
    struct FakeTracker {
        state: Mutex<FakeState>,
    }

    impl FakeTracker {
        fn with_searches(searches: Vec<Vec<FeedbackIssueRecord>>) -> Self {
            Self {
                state: Mutex::new(FakeState {
                    searches: searches.into(),
                    ..FakeState::default()
                }),
            }
        }

        fn add_view(&self, record: FeedbackIssueRecord) {
            self.state
                .lock()
                .unwrap()
                .views
                .insert(record.candidate.number, record);
        }
    }

    impl FeedbackIssueTracker for FakeTracker {
        fn search_open(
            &self,
            _repo: &str,
            query: &str,
            _fields: &[FeedbackIssueSearchField],
            _limit: usize,
        ) -> Result<Vec<FeedbackIssueRecord>, FeedbackIssueTrackerError> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(format!("search:{query}"));
            Ok(state.searches.pop_front().unwrap_or_default())
        }

        fn view_issue(
            &self,
            _repo: &str,
            number: u64,
        ) -> Result<FeedbackIssueRecord, FeedbackIssueTrackerError> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(format!("view:{number}"));
            state
                .views
                .get(&number)
                .cloned()
                .ok_or_else(|| FeedbackIssueTrackerError {
                    message: format!("missing fake issue {number}"),
                })
        }

        fn comment_issue(
            &self,
            _repo: &str,
            number: u64,
            body: &str,
        ) -> Result<(), FeedbackIssueTrackerError> {
            assert!(body.contains("Agent re-observation"));
            let mut state = self.state.lock().unwrap();
            state.calls.push(format!("comment:{number}"));
            state.comments.push(number);
            Ok(())
        }

        fn create_issue(
            &self,
            repo: &str,
            title: &str,
            body: &str,
        ) -> Result<FeedbackIssueCandidate, FeedbackIssueTrackerError> {
            assert!(title.starts_with("agent report:"));
            assert!(body.contains("dcc-mcp-finding:"));
            let mut state = self.state.lock().unwrap();
            state.calls.push("create".to_string());
            state.creates += 1;
            Ok(candidate(repo, 77))
        }
    }

    fn candidate(repo: &str, number: u64) -> FeedbackIssueCandidate {
        FeedbackIssueCandidate {
            number,
            title: format!("Issue {number}"),
            url: format!("https://github.com/{repo}/issues/{number}"),
        }
    }

    fn record(repo: &str, number: u64, fingerprint: &str) -> FeedbackIssueRecord {
        FeedbackIssueRecord {
            candidate: candidate(repo, number),
            state: FeedbackIssueState::Open,
            body: format!("<!-- dcc-mcp-finding:{fingerprint} -->"),
        }
    }

    fn finding(mode: FindingRedactionMode) -> FindingV1 {
        let repo = "dcc-mcp/dcc-mcp-godot";
        let mut value = FindingV1 {
            schema_version: FINDING_V1_SCHEMA_VERSION,
            fingerprint: String::new(),
            dcc_type: "godot".to_string(),
            adapter: "dcc-mcp-godot".to_string(),
            adapter_version: "0.3.0".to_string(),
            core_version: "0.20.13".to_string(),
            host_version: "4.4.1".to_string(),
            os: "windows".to_string(),
            phase: FindingPhase::Startup,
            severity: FindingSeverity::Blocker,
            tool_slug: None,
            intent: "Start the Godot adapter".to_string(),
            observed: "The bridge did not start".to_string(),
            expected: "The bridge becomes ready".to_string(),
            repro: FindingReproV1 {
                argv: vec!["dcc-mcp-cli".to_string(), "status".to_string()],
                steps: Vec::new(),
            },
            evidence: FindingEvidenceV1 {
                error_kind: Some("startup_failed".to_string()),
                extra: BTreeMap::from([("ignored".to_string(), json!("private"))]),
                ..FindingEvidenceV1::default()
            },
            redaction_status: FindingRedactionStatusV1 {
                mode,
                redaction_markers_detected: false,
                raw_payloads_excluded: true,
                prompts_excluded: true,
                scripts_excluded: true,
                auth_material_excluded: true,
                local_urls_excluded: true,
                absolute_paths_excluded: true,
                private_identifiers_excluded: true,
            },
        };
        value.fingerprint = finding_fingerprint(
            repo,
            value.phase,
            value.tool_slug.as_deref(),
            value.evidence.error_kind.as_deref(),
            &value.host_version,
        )
        .unwrap();
        value
    }

    fn request(finding: &FindingV1) -> (tempfile::TempDir, FeedbackFileRequest) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("finding.json");
        std::fs::write(&path, serde_json::to_vec_pretty(finding).unwrap()).unwrap();
        (
            temp,
            FeedbackFileRequest {
                finding_path: path,
                catalog_path: None,
                decision: None,
                authorized: false,
                authorization: None,
            },
        )
    }

    fn authorize(request: &mut FeedbackFileRequest, decision: FeedbackFileDecision) {
        request.authorization = Some(FeedbackFileInputs::capture(request).unwrap().authorization);
        request.decision = Some(decision);
        request.authorized = true;
    }

    fn argv_value<'a>(argv: &'a [String], flag: &str) -> &'a str {
        let index = argv.iter().position(|arg| arg == flag).unwrap();
        argv.get(index + 1).map(String::as_str).unwrap()
    }

    fn replay_request(step: &FeedbackFileNextStep) -> FeedbackFileRequest {
        let argv = &step.argv;
        let decision = if let Some(index) = argv.iter().position(|arg| arg == "--existing") {
            FeedbackFileDecision::Existing(argv[index + 1].parse().unwrap())
        } else {
            assert!(argv.iter().any(|arg| arg == "--create"));
            FeedbackFileDecision::Create
        };
        let catalog_path = argv
            .iter()
            .position(|arg| arg == "--catalog")
            .map(|index| PathBuf::from(&argv[index + 1]));
        FeedbackFileRequest {
            finding_path: PathBuf::from(&argv[3]),
            catalog_path,
            decision: Some(decision),
            authorized: true,
            authorization: Some(FeedbackFileAuthorization {
                canonical_finding_path: PathBuf::from(argv_value(argv, "--expected-finding-path")),
                finding_sha256: argv_value(argv, "--expected-finding-sha256").to_string(),
                fingerprint: argv_value(argv, "--expected-fingerprint").to_string(),
                repo: argv_value(argv, "--expected-repo").to_string(),
                catalog_identity: FeedbackCatalogIdentity::from_plan_value(argv_value(
                    argv,
                    "--expected-catalog-path",
                )),
                catalog_sha256: argv_value(argv, "--expected-catalog-sha256").to_string(),
            }),
        }
    }

    fn finding_with_exact_comment_body_chars(target_chars: usize) -> FindingV1 {
        let route = crate::domain::feedback::FeedbackRoute {
            repo: "dcc-mcp/dcc-mcp-godot".to_string(),
            issues_url: "https://github.com/dcc-mcp/dcc-mcp-godot/issues".to_string(),
            rationale: crate::domain::feedback::FeedbackRouteRationale::AdapterPhase,
        };
        let mut value = finding(FindingRedactionMode::PublicSafe);
        value.repro.argv.clear();
        value.repro.steps = vec!["界".to_string(); 16];
        let baseline = build_issue_document(&value, &route)
            .unwrap()
            .comment_body
            .chars()
            .count();
        let mut remaining = target_chars.checked_sub(baseline).unwrap();
        for step in &mut value.repro.steps {
            let added = remaining.min(4_095);
            step.push_str(&"界".repeat(added));
            remaining -= added;
        }
        assert_eq!(remaining, 0, "target must fit the Finding v1 bounds");
        assert!(value.validate().is_ok());
        value
    }

    #[test]
    fn public_safe_validation_happens_before_tracker_access() {
        let tracker = FakeTracker::default();
        let (_temp, request) = request(&finding(FindingRedactionMode::NeedsReview));

        let error = FeedbackFileService::new(&tracker)
            .file(request)
            .unwrap_err();

        assert!(matches!(error, FeedbackFileServiceError::Document(_)));
        assert!(tracker.state.lock().unwrap().calls.is_empty());
    }

    #[test]
    fn unauthorized_exact_match_emits_one_executable_comment_step() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let existing = record("dcc-mcp/dcc-mcp-godot", 42, &finding.fingerprint);
        let tracker = FakeTracker::with_searches(vec![vec![existing]]);
        let (_temp, request) = request(&finding);
        let canonical_finding = std::fs::canonicalize(&request.finding_path).unwrap();

        let result = FeedbackFileService::new(&tracker).file(request).unwrap();

        assert_eq!(result.status, FeedbackFileStatus::Planned);
        assert!(!result.authorized);
        assert_eq!(
            result.recommendation,
            FeedbackFilingRecommendation::CommentExisting { issue_number: 42 }
        );
        let argv = &result.next_step.unwrap().argv;
        assert_eq!(PathBuf::from(&argv[3]), canonical_finding);
        for expected in [
            "--expected-finding-path",
            "--expected-finding-sha256",
            "--expected-fingerprint",
            "--expected-repo",
            "--expected-catalog-sha256",
        ] {
            assert!(argv.iter().any(|arg| arg == expected), "missing {expected}");
        }
        assert!(argv.windows(2).any(|pair| pair == ["--existing", "42"]));
        assert!(argv.iter().any(|arg| arg == "--yes"));
        let state = tracker.state.lock().unwrap();
        assert!(state.comments.is_empty());
        assert_eq!(state.creates, 0);
        assert_eq!(
            state.calls[0],
            format!(
                "search:\"{}\"",
                finding.fingerprint.strip_prefix("sha256:").unwrap()
            )
        );
    }

    #[test]
    fn fuzzy_candidates_require_explicit_review() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let fuzzy = record("dcc-mcp/dcc-mcp-godot", 99, "sha256:other");
        let tracker = FakeTracker::with_searches(vec![vec![], vec![fuzzy]]);
        let (_temp, request) = request(&finding);

        let result = FeedbackFileService::new(&tracker).file(request).unwrap();

        assert_eq!(
            result.recommendation,
            FeedbackFilingRecommendation::ReviewRequired
        );
        assert!(result.next_step.is_none());
        assert_eq!(result.review_options.len(), 2);
    }

    #[test]
    fn keyword_search_reports_remote_truncation_before_local_deduplication() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let duplicate = record("dcc-mcp/dcc-mcp-godot", 99, "sha256:other");
        let tracker = FakeTracker::with_searches(vec![vec![], vec![duplicate; 11]]);
        let (_temp, request) = request(&finding);

        let result = FeedbackFileService::new(&tracker).file(request).unwrap();

        assert!(result.dedup.keyword_truncated);
        assert_eq!(result.dedup.keyword.len(), 1);
    }

    #[test]
    fn authorized_create_rechecks_fingerprint_and_stops_on_race() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let raced = record("dcc-mcp/dcc-mcp-godot", 42, &finding.fingerprint);
        let tracker = FakeTracker::with_searches(vec![vec![], vec![], vec![raced]]);
        let (_temp, mut request) = request(&finding);
        authorize(&mut request, FeedbackFileDecision::Create);

        let error = FeedbackFileService::new(&tracker)
            .file(request)
            .unwrap_err();

        assert!(matches!(
            error,
            FeedbackFileServiceError::ExactIssueConflict { .. }
        ));
        assert_eq!(tracker.state.lock().unwrap().creates, 0);
    }

    #[test]
    fn authorized_create_writes_only_after_an_empty_exact_recheck() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let tracker = FakeTracker::with_searches(vec![vec![], vec![], vec![]]);
        let (_temp, mut request) = request(&finding);
        authorize(&mut request, FeedbackFileDecision::Create);

        let result = FeedbackFileService::new(&tracker).file(request).unwrap();

        assert_eq!(result.status, FeedbackFileStatus::Created);
        assert_eq!(result.issue.unwrap().number, 77);
        assert_eq!(tracker.state.lock().unwrap().creates, 1);
    }

    #[test]
    fn authorized_comment_rechecks_and_writes_only_selected_open_issue() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let existing = record("dcc-mcp/dcc-mcp-godot", 42, &finding.fingerprint);
        let tracker =
            FakeTracker::with_searches(vec![vec![existing.clone()], vec![existing.clone()]]);
        tracker.add_view(existing);
        let (_temp, mut request) = request(&finding);
        authorize(&mut request, FeedbackFileDecision::Existing(42));

        let result = FeedbackFileService::new(&tracker).file(request).unwrap();

        assert_eq!(result.status, FeedbackFileStatus::Commented);
        assert_eq!(result.issue.unwrap().number, 42);
        let state = tracker.state.lock().unwrap();
        assert_eq!(state.comments, vec![42]);
        assert_eq!(state.creates, 0);
        assert_eq!(
            state
                .calls
                .iter()
                .filter(|call| call.starts_with("search:"))
                .count(),
            2
        );
    }

    #[test]
    fn explicit_authorization_resolves_multiple_exact_matches_for_commenting() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let first = record("dcc-mcp/dcc-mcp-godot", 42, &finding.fingerprint);
        let selected = record("dcc-mcp/dcc-mcp-godot", 43, &finding.fingerprint);
        let tracker = FakeTracker::with_searches(vec![
            vec![first.clone(), selected.clone()],
            vec![first, selected.clone()],
        ]);
        tracker.add_view(selected);
        let (_temp, mut request) = request(&finding);
        authorize(&mut request, FeedbackFileDecision::Existing(43));

        let result = FeedbackFileService::new(&tracker).file(request).unwrap();

        assert_eq!(result.status, FeedbackFileStatus::Commented);
        assert_eq!(result.issue.unwrap().number, 43);
        assert_eq!(tracker.state.lock().unwrap().comments, vec![43]);
    }

    #[test]
    fn authorized_comment_rejects_a_closed_selected_issue() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let open = record("dcc-mcp/dcc-mcp-godot", 42, &finding.fingerprint);
        let mut closed = open.clone();
        closed.state = FeedbackIssueState::Closed;
        let tracker = FakeTracker::with_searches(vec![vec![open.clone()], vec![open]]);
        tracker.add_view(closed);
        let (_temp, mut request) = request(&finding);
        authorize(&mut request, FeedbackFileDecision::Existing(42));

        let error = FeedbackFileService::new(&tracker)
            .file(request)
            .unwrap_err();

        assert!(matches!(
            error,
            FeedbackFileServiceError::IssueNotOpen { number: 42 }
        ));
        assert!(tracker.state.lock().unwrap().comments.is_empty());
    }

    #[test]
    fn authorization_without_decision_stops_before_tracker_access() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let tracker = FakeTracker::default();
        let (_temp, mut request) = request(&finding);
        request.authorized = true;

        let error = FeedbackFileService::new(&tracker)
            .file(request)
            .unwrap_err();

        assert!(matches!(
            error,
            FeedbackFileServiceError::AuthorizationDecisionRequired
        ));
        assert!(tracker.state.lock().unwrap().calls.is_empty());
    }

    #[test]
    fn multibyte_unicode_body_boundary_accepts_65536_and_rejects_65537_before_tracker_io() {
        let route = crate::domain::feedback::FeedbackRoute {
            repo: "dcc-mcp/dcc-mcp-godot".to_string(),
            issues_url: "https://github.com/dcc-mcp/dcc-mcp-godot/issues".to_string(),
            rationale: crate::domain::feedback::FeedbackRouteRationale::AdapterPhase,
        };
        let accepted = finding_with_exact_comment_body_chars(65_536);
        let document = build_issue_document(&accepted, &route).unwrap();
        assert_eq!(document.comment_body.chars().count(), 65_536);
        assert!(document.comment_body.contains('界'));
        let accepted_tracker = FakeTracker::default();
        let (_temp, accepted_request) = request(&accepted);
        assert!(
            FeedbackFileService::new(&accepted_tracker)
                .file(accepted_request)
                .is_ok()
        );

        let mut rejected = accepted;
        rejected.repro.steps.last_mut().unwrap().push('界');
        assert!(rejected.validate().is_ok());
        let rejected_tracker = FakeTracker::default();
        let (_temp, rejected_request) = request(&rejected);

        let error = FeedbackFileService::new(&rejected_tracker)
            .file(rejected_request)
            .unwrap_err();

        assert!(matches!(
            error,
            FeedbackFileServiceError::Document(FeedbackFileError::BodyTooLarge {
                kind: "comment body",
                actual_chars: 65_537,
                max_chars: 65_536,
            })
        ));
        assert!(rejected_tracker.state.lock().unwrap().calls.is_empty());
    }

    #[test]
    fn maximum_valid_finding_body_is_rejected_before_tracker_io() {
        let mut finding = finding(FindingRedactionMode::PublicSafe);
        finding.repro.argv.clear();
        finding.repro.steps = vec!["x".repeat(4_096); 64];
        assert!(finding.validate().is_ok());
        let tracker = FakeTracker::default();
        let (_temp, request) = request(&finding);

        let error = FeedbackFileService::new(&tracker)
            .file(request)
            .unwrap_err();

        assert!(matches!(error, FeedbackFileServiceError::Document(_)));
        assert!(tracker.state.lock().unwrap().calls.is_empty());
    }

    #[test]
    fn authorized_replay_rejects_finding_content_drift_before_tracker_io() {
        let original = finding(FindingRedactionMode::PublicSafe);
        let plan_tracker = FakeTracker::with_searches(vec![vec![], vec![]]);
        let (_temp, request) = request(&original);
        let finding_path = request.finding_path.clone();
        let plan = FeedbackFileService::new(&plan_tracker)
            .file(request)
            .unwrap();
        let replay = replay_request(plan.next_step.as_ref().unwrap());

        let mut changed = original;
        changed.observed = "A different reviewed observation".to_string();
        std::fs::write(&finding_path, serde_json::to_vec_pretty(&changed).unwrap()).unwrap();
        let tracker = FakeTracker::default();

        let error = FeedbackFileService::new(&tracker).file(replay).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("authorization no longer matches")
        );
        assert!(tracker.state.lock().unwrap().calls.is_empty());
    }

    #[test]
    fn authorized_replay_from_plan_writes_unchanged_inputs() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let plan_tracker = FakeTracker::with_searches(vec![vec![], vec![]]);
        let (_temp, request) = request(&finding);
        let plan = FeedbackFileService::new(&plan_tracker)
            .file(request)
            .unwrap();
        let replay = replay_request(plan.next_step.as_ref().unwrap());
        let tracker = FakeTracker::with_searches(vec![vec![], vec![], vec![]]);

        let result = FeedbackFileService::new(&tracker).file(replay).unwrap();

        assert_eq!(result.status, FeedbackFileStatus::Created);
        assert_eq!(tracker.state.lock().unwrap().creates, 1);
    }

    #[test]
    fn authorized_replay_rejects_catalog_drift_before_tracker_io() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let plan_tracker = FakeTracker::with_searches(vec![vec![], vec![]]);
        let (temp, mut request) = request(&finding);
        let source_catalog =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dcc-mcp-catalog.yml");
        let catalog_path = temp.path().join("catalog.yml");
        std::fs::copy(source_catalog, &catalog_path).unwrap();
        request.catalog_path = Some(catalog_path.clone());
        let plan = FeedbackFileService::new(&plan_tracker)
            .file(request)
            .unwrap();
        let replay = replay_request(plan.next_step.as_ref().unwrap());

        let mut catalog = std::fs::read_to_string(&catalog_path).unwrap();
        catalog.push_str("\n# authorization drift\n");
        std::fs::write(&catalog_path, catalog).unwrap();
        let tracker = FakeTracker::default();

        let error = FeedbackFileService::new(&tracker).file(replay).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("authorization no longer matches")
        );
        assert!(tracker.state.lock().unwrap().calls.is_empty());
    }

    #[test]
    fn authorized_replay_rejects_same_bytes_at_a_different_catalog_path_before_tracker_io() {
        let finding = finding(FindingRedactionMode::PublicSafe);
        let plan_tracker = FakeTracker::with_searches(vec![vec![], vec![]]);
        let (temp, mut request) = request(&finding);
        let source_catalog =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dcc-mcp-catalog.yml");
        let catalog_a = temp.path().join("catalog-a.yml");
        let catalog_b = temp.path().join("catalog-b.yml");
        std::fs::copy(&source_catalog, &catalog_a).unwrap();
        std::fs::copy(&source_catalog, &catalog_b).unwrap();
        request.catalog_path = Some(catalog_a.clone());
        let plan = FeedbackFileService::new(&plan_tracker)
            .file(request)
            .unwrap();
        assert_eq!(
            argv_value(
                &plan.next_step.as_ref().unwrap().argv,
                "--expected-catalog-path"
            ),
            std::fs::canonicalize(&catalog_a).unwrap().to_str().unwrap()
        );
        let mut replay = replay_request(plan.next_step.as_ref().unwrap());
        replay.catalog_path = Some(catalog_b);
        let tracker = FakeTracker::default();

        let error = FeedbackFileService::new(&tracker).file(replay).unwrap_err();

        assert!(matches!(
            error,
            FeedbackFileServiceError::AuthorizationMismatch
        ));
        assert!(tracker.state.lock().unwrap().calls.is_empty());
    }

    #[test]
    fn authorized_replay_rejects_cwd_default_catalog_swap_before_tracker_io() {
        let temp = tempfile::tempdir().unwrap();
        let plan_dir = temp.path().join("plan");
        let replay_dir = temp.path().join("replay");
        std::fs::create_dir_all(&plan_dir).unwrap();
        std::fs::create_dir_all(&replay_dir).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "application::feedback_file::tests::cwd_default_catalog_swap_helper",
            ])
            .env("DCC_MCP_FEEDBACK_CWD_SWAP_ROOT", temp.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "helper failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn cwd_default_catalog_swap_helper() {
        let Some(root) = std::env::var_os("DCC_MCP_FEEDBACK_CWD_SWAP_ROOT").map(PathBuf::from)
        else {
            return;
        };
        let plan_dir = root.join("plan");
        let replay_dir = root.join("replay");
        let source_catalog =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dcc-mcp-catalog.yml");
        std::fs::copy(source_catalog, replay_dir.join(DEFAULT_CATALOG_PATH)).unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&plan_dir).unwrap();

        let finding = finding(FindingRedactionMode::PublicSafe);
        let plan_tracker = FakeTracker::with_searches(vec![vec![], vec![]]);
        let (_temp, request) = request(&finding);
        let plan = FeedbackFileService::new(&plan_tracker)
            .file(request)
            .unwrap();
        assert!(
            !plan
                .next_step
                .as_ref()
                .unwrap()
                .argv
                .contains(&"--catalog".to_string())
        );
        assert_eq!(
            argv_value(
                &plan.next_step.as_ref().unwrap().argv,
                "--expected-catalog-path"
            ),
            BUNDLED_CATALOG_SENTINEL
        );
        let replay = replay_request(plan.next_step.as_ref().unwrap());
        std::env::set_current_dir(&replay_dir).unwrap();
        let tracker = FakeTracker::default();

        let result = FeedbackFileService::new(&tracker).file(replay);

        std::env::set_current_dir(original_dir).unwrap();
        assert!(matches!(
            result,
            Err(FeedbackFileServiceError::AuthorizationMismatch)
        ));
        assert!(tracker.state.lock().unwrap().calls.is_empty());
    }
}
