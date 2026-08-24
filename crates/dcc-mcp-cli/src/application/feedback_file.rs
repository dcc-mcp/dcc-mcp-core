use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

use crate::domain::feedback_file::{
    FEEDBACK_FILE_SCHEMA_VERSION, FeedbackFileError, FeedbackFilingRecommendation,
    FeedbackIssueCandidate, build_issue_document, recommend_filing,
};

use super::feedback::{FeedbackRouteService, read_finding};

const DEFAULT_CATALOG_PATH: &str = "dcc-mcp-catalog.yml";
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

        let finding = read_finding(&request.finding_path)?;
        let route = FeedbackRouteService::new(PathBuf::from(DEFAULT_CATALOG_PATH))
            .route(&request.finding_path, request.catalog_path.as_deref())?;
        let document = build_issue_document(&finding, &route)?;
        let exact_records = self.search_exact(&route.repo, &finding.fingerprint)?;
        let exact = candidates(&exact_records);
        let (keyword, keyword_truncated) = if exact.is_empty() {
            self.search_keywords(&route.repo, &document.search_terms)?
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
            let (next_step, review_options) =
                plan_next_steps(&request, recommendation.clone(), &dedup)?;
            return Ok(FeedbackFileResult {
                schema_version: FEEDBACK_FILE_SCHEMA_VERSION,
                authorized: false,
                status: FeedbackFileStatus::Planned,
                repo: route.repo,
                fingerprint: finding.fingerprint,
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
                let raced = candidates(&self.search_exact(&route.repo, &finding.fingerprint)?);
                if !raced.is_empty() {
                    return Err(exact_conflict(&raced));
                }
                self.tracker
                    .create_issue(&route.repo, &document.title, &document.body)?
            }
            FeedbackFileDecision::Existing(number) => {
                let raced = candidates(&self.search_exact(&route.repo, &finding.fingerprint)?);
                if !raced.is_empty() && !raced.iter().any(|issue| issue.number == number) {
                    return Err(exact_conflict(&raced));
                }
                let issue = self.tracker.view_issue(&route.repo, number)?;
                if issue.state != FeedbackIssueState::Open {
                    return Err(FeedbackFileServiceError::IssueNotOpen { number });
                }
                self.tracker
                    .comment_issue(&route.repo, number, &document.comment_body)?;
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
            repo: route.repo,
            fingerprint: finding.fingerprint,
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
    request: &FeedbackFileRequest,
    recommendation: FeedbackFilingRecommendation,
    dedup: &FeedbackFileDedup,
) -> Result<(Option<FeedbackFileNextStep>, Vec<FeedbackFileNextStep>), FeedbackFileServiceError> {
    if let Some(decision) = request.decision {
        return Ok((Some(next_step(request, decision)?), Vec::new()));
    }
    match recommendation {
        FeedbackFilingRecommendation::CommentExisting { issue_number } => Ok((
            Some(next_step(
                request,
                FeedbackFileDecision::Existing(issue_number),
            )?),
            Vec::new(),
        )),
        FeedbackFilingRecommendation::Create => Ok((
            Some(next_step(request, FeedbackFileDecision::Create)?),
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
                .map(|issue| next_step(request, FeedbackFileDecision::Existing(issue.number)))
                .collect::<Result<Vec<_>, _>>()?;
            if dedup.exact.is_empty() {
                options.push(next_step(request, FeedbackFileDecision::Create)?);
            }
            Ok((None, options))
        }
    }
}

fn next_step(
    request: &FeedbackFileRequest,
    decision: FeedbackFileDecision,
) -> Result<FeedbackFileNextStep, FeedbackFileServiceError> {
    let finding_path = request
        .finding_path
        .to_str()
        .ok_or(FeedbackFileServiceError::NonUnicodePath)?;
    let mut argv = vec![
        "dcc-mcp-cli".to_string(),
        "feedback".to_string(),
        "file".to_string(),
        finding_path.to_string(),
    ];
    if let Some(path) = request.catalog_path.as_deref() {
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
    argv.extend(["--yes".to_string(), "--json".to_string()]);
    Ok(FeedbackFileNextStep { argv })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap, VecDeque};
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
            },
        )
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

        let result = FeedbackFileService::new(&tracker).file(request).unwrap();

        assert_eq!(result.status, FeedbackFileStatus::Planned);
        assert!(!result.authorized);
        assert_eq!(
            result.recommendation,
            FeedbackFilingRecommendation::CommentExisting { issue_number: 42 }
        );
        let argv = &result.next_step.unwrap().argv;
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
        request.authorized = true;
        request.decision = Some(FeedbackFileDecision::Create);

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
        request.authorized = true;
        request.decision = Some(FeedbackFileDecision::Create);

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
        request.authorized = true;
        request.decision = Some(FeedbackFileDecision::Existing(42));

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
        request.authorized = true;
        request.decision = Some(FeedbackFileDecision::Existing(43));

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
        request.authorized = true;
        request.decision = Some(FeedbackFileDecision::Existing(42));

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
}
