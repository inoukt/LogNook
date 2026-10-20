use crate::{AppState, commands::oem_catalog::execute_candidate_try_bound};
use lognook_core::{OemCandidate, OemCandidateOutcome, OemCatalog, OemCatalogPage};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    thread,
};
use tauri::{AppHandle, Manager};

const MAX_VISIBLE_CANDIDATES: usize = 100;
const PAGE_SIZE: usize = 100;

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OemDiscoveryFilter {
    pub query: String,
    pub brand: String,
    pub model: String,
    pub service: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OemDiscoveryRequest {
    Status {
        #[serde(default)]
        candidate_ids: Vec<String>,
    },
    SetAutoTry {
        enabled: bool,
        adapter_id: String,
        connection_generation: u64,
    },
    EnqueueVisible {
        candidate_ids: Vec<String>,
        adapter_id: String,
        connection_generation: u64,
    },
    TryFiltered {
        filter: OemDiscoveryFilter,
        adapter_id: String,
        connection_generation: u64,
    },
    Stop {},
    Search {
        filter: OemDiscoveryFilter,
        offset: usize,
        limit: usize,
        values_first: bool,
    },
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct OemDiscoveryStatus {
    pub adapter_id: Option<String>,
    pub connection_generation: Option<u64>,
    pub auto_try_visible: bool,
    pub running: bool,
    pub queued: usize,
    pub completed: usize,
    pub with_values: usize,
    pub current_candidate_id: Option<String>,
    pub last_error: Option<String>,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OemDiscoveryResultState {
    Queued,
    Running,
    Value,
    Reply,
    NoResponse,
    Negative,
    Error,
    Unsupported,
}

#[derive(Clone, Debug, Serialize)]
pub struct OemDiscoveryResult {
    pub candidate_id: String,
    pub state: OemDiscoveryResultState,
    pub outcome: Option<String>,
    pub value: Option<f64>,
    pub unit: String,
    pub detail: Option<String>,
    pub source_blob_sha: String,
    pub evidence_saved: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct OemDiscoveryResponse {
    pub status: OemDiscoveryStatus,
    pub page: Option<OemCatalogPage>,
    pub results: Vec<OemDiscoveryResult>,
}

#[derive(Clone, Debug, Default)]
pub struct OemDiscoveryService {
    state: Arc<Mutex<DiscoveryState>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DiscoverySession {
    adapter_id: String,
    connection_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Ui,
    Agent,
}

#[derive(Clone, Debug)]
struct QueuedCandidate {
    session: DiscoverySession,
    candidate_id: String,
    source_blob_sha: String,
    unit: String,
}

#[derive(Clone, Debug, Default)]
struct DiscoveryState {
    session: Option<DiscoverySession>,
    auto_try_visible: bool,
    running: bool,
    queue: VecDeque<QueuedCandidate>,
    seen: HashSet<(String, String)>,
    results: HashMap<String, OemDiscoveryResult>,
    current_candidate_id: Option<String>,
    last_error: Option<String>,
    owner: Option<Owner>,
    filtered: Option<FilteredRun>,
    next_filtered_token: u64,
    revision: u64,
}

#[derive(Clone, Debug)]
struct FilteredRun {
    filter: OemDiscoveryFilter,
    offset: usize,
    total: usize,
    token: u64,
    session: DiscoverySession,
}

#[tauri::command]
pub async fn oem_discovery(
    app: AppHandle,
    request: OemDiscoveryRequest,
) -> Result<OemDiscoveryResponse, String> {
    execute(&app, request, false).await
}

pub async fn execute(
    app: &AppHandle,
    request: OemDiscoveryRequest,
    from_agent: bool,
) -> Result<OemDiscoveryResponse, String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || execute_blocking(&app, request, from_agent))
        .await
        .map_err(|error| format!("OEM discovery command worker failed: {error}"))?
}

fn execute_blocking(
    app: &AppHandle,
    request: OemDiscoveryRequest,
    from_agent: bool,
) -> Result<OemDiscoveryResponse, String> {
    if from_agent && !agent_access_unlocked(app) {
        return Err("Agent Access is locked; unlock it in LogNook Settings".into());
    }
    let service = app.state::<OemDiscoveryService>();
    match request {
        OemDiscoveryRequest::Status { candidate_ids } => {
            validate_candidate_ids(&candidate_ids, false)?;
            refresh_session(app, &service);
            Ok(response(&service, &candidate_ids, None))
        }
        OemDiscoveryRequest::SetAutoTry {
            enabled,
            adapter_id,
            connection_generation,
        } => {
            let session = check_session(app, &adapter_id, connection_generation)?;
            let mut state = lock_state(&service)?;
            reset_session(&mut state, Some(session));
            if enabled {
                reserve_owner(&mut state, owner(from_agent))?;
            } else if state
                .owner
                .is_some_and(|active| active != owner(from_agent))
            {
                return Err("Another discovery owner has pending requests".into());
            }
            state.auto_try_visible = enabled;
            if !enabled {
                cancel_pending(&mut state);
                if !state.running {
                    state.owner = None;
                }
            }
            state.revision += 1;
            drop(state);
            Ok(response(&service, &[], None))
        }
        OemDiscoveryRequest::EnqueueVisible {
            candidate_ids,
            adapter_id,
            connection_generation,
        } => {
            validate_candidate_ids(&candidate_ids, true)?;
            let session = check_session(app, &adapter_id, connection_generation)?;
            let catalog = catalog(app)?;
            let candidates = candidate_ids
                .iter()
                .map(|id| catalog.candidate_for_discovery(id))
                .collect::<Result<Vec<_>, _>>()?;
            {
                let mut state = lock_state(&service)?;
                reset_session(&mut state, Some(session.clone()));
                if !state.auto_try_visible {
                    return Err("Enable visible-row auto-try before queuing candidates".into());
                }
                reserve_owner(&mut state, owner(from_agent))?;
                for candidate in candidates {
                    enqueue_candidate(&mut state, &session, &candidate);
                }
                state.revision += 1;
            }
            start_worker(
                app.clone(),
                service.inner().state.clone(),
                owner(from_agent),
            )?;
            Ok(response(&service, &candidate_ids, None))
        }
        OemDiscoveryRequest::TryFiltered {
            filter,
            adapter_id,
            connection_generation,
        } => {
            validate_filter(&filter)?;
            let session = check_session(app, &adapter_id, connection_generation)?;
            let catalog = catalog(app)?;
            let first = catalog.search(
                &filter.query,
                &filter.brand,
                &filter.model,
                filter.service.as_deref(),
                0,
                1,
            )?;
            {
                let mut state = lock_state(&service)?;
                reset_session(&mut state, Some(session.clone()));
                reserve_owner(&mut state, owner(from_agent))?;
                state.next_filtered_token += 1;
                state.filtered = Some(FilteredRun {
                    filter,
                    offset: 0,
                    total: first.total,
                    token: state.next_filtered_token,
                    session,
                });
                state.last_error = None;
                state.revision += 1;
            }
            start_worker(
                app.clone(),
                service.inner().state.clone(),
                owner(from_agent),
            )?;
            Ok(response(&service, &[], None))
        }
        OemDiscoveryRequest::Stop {} => {
            let mut state = lock_state(&service)?;
            if state
                .owner
                .is_some_and(|active| active != owner(from_agent))
            {
                return Err("Another discovery owner has pending requests".into());
            }
            cancel_pending(&mut state);
            if !state.running {
                state.owner = None;
            }
            state.revision += 1;
            drop(state);
            Ok(response(&service, &[], None))
        }
        OemDiscoveryRequest::Search {
            filter,
            offset,
            limit,
            values_first,
        } => {
            validate_filter(&filter)?;
            refresh_session(app, &service);
            let priority = if values_first {
                priority_candidate_ids(&lock_state(&service)?.results)
            } else {
                Vec::new()
            };
            let page = catalog(app)?.search_prioritized(
                &filter.query,
                &filter.brand,
                &filter.model,
                filter.service.as_deref(),
                offset,
                limit,
                &priority,
            )?;
            let ids = page
                .candidates
                .iter()
                .map(|candidate| candidate.candidate_id.clone())
                .collect::<Vec<_>>();
            Ok(response(&service, &ids, Some(page)))
        }
    }
}

pub fn stop_agent_owned(app: &AppHandle) -> Result<(), String> {
    let service = app.state::<OemDiscoveryService>();
    let mut state = lock_state(&service)?;
    if state.owner == Some(Owner::Agent) {
        cancel_pending(&mut state);
        state.revision += 1;
        if !state.running {
            state.owner = None;
        }
    }
    Ok(())
}

fn owner(from_agent: bool) -> Owner {
    if from_agent { Owner::Agent } else { Owner::Ui }
}

fn validate_candidate_ids(ids: &[String], nonempty: bool) -> Result<(), String> {
    if (nonempty && ids.is_empty()) || ids.len() > MAX_VISIBLE_CANDIDATES {
        return Err(format!(
            "Choose between 1 and {MAX_VISIBLE_CANDIDATES} visible OEM candidates"
        ));
    }
    if ids.iter().any(|id| id.trim().is_empty() || id.len() > 128) {
        return Err("Choose valid OEM candidate IDs".into());
    }
    Ok(())
}

fn validate_filter(filter: &OemDiscoveryFilter) -> Result<(), String> {
    if [&filter.query, &filter.brand, &filter.model]
        .into_iter()
        .any(|value| value.len() > 256)
    {
        return Err("OEM discovery filters must be at most 256 characters".into());
    }
    if filter
        .service
        .as_deref()
        .is_some_and(|service| !matches!(service, "21" | "22"))
    {
        return Err("OEM catalog filters support read-only services 21 and 22".into());
    }
    Ok(())
}

fn catalog(app: &AppHandle) -> Result<OemCatalog, String> {
    Ok(OemCatalog::new(
        app.state::<AppState>().pid_catalog_path.clone(),
    ))
}

fn check_session(
    app: &AppHandle,
    adapter_id: &str,
    connection_generation: u64,
) -> Result<DiscoverySession, String> {
    if adapter_id.trim().is_empty() || adapter_id.len() > 512 {
        return Err("Choose the connected adapter ID".into());
    }
    let app_state = app.state::<AppState>();
    let engine = app_state
        .engine
        .lock()
        .map_err(|_| "Acquisition engine lock is unavailable".to_owned())?;
    let capabilities = engine
        .connection_capabilities()
        .ok_or_else(|| "No OBD adapter is connected".to_owned())?;
    if engine.obd_connection_generation() != connection_generation {
        return Err("OBD connection changed before OEM discovery".into());
    }
    if capabilities.device.stable_id != adapter_id {
        return Err("The connected adapter changed before OEM discovery".into());
    }
    Ok(DiscoverySession {
        adapter_id: adapter_id.into(),
        connection_generation,
    })
}

fn agent_access_unlocked(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .agent_access
        .lock()
        .is_ok_and(|mut access| access.status().unlocked)
}

fn refresh_session(app: &AppHandle, service: &OemDiscoveryService) {
    let app_state = app.state::<AppState>();
    let Ok(engine) = app_state.engine.try_lock() else {
        return;
    };
    let session = engine
        .connection_capabilities()
        .map(|capabilities| DiscoverySession {
            adapter_id: capabilities.device.stable_id.clone(),
            connection_generation: engine.obd_connection_generation(),
        });
    if let Ok(mut state) = service.state.lock() {
        if state.session != session {
            reset_session(&mut state, session);
        }
    }
}

fn refresh_live_session(app: &AppHandle, service: &OemDiscoveryService) {
    let app_state = app.state::<AppState>();
    let session = app_state.engine.lock().ok().and_then(|engine| {
        engine
            .connection_capabilities()
            .map(|capabilities| DiscoverySession {
                adapter_id: capabilities.device.stable_id.clone(),
                connection_generation: engine.obd_connection_generation(),
            })
    });
    if let Ok(mut state) = service.state.lock() {
        reset_session(&mut state, session);
    }
}

fn reserve_owner(state: &mut DiscoveryState, owner: Owner) -> Result<(), String> {
    if !state.running
        && !state.auto_try_visible
        && state.queue.is_empty()
        && state.filtered.is_none()
    {
        state.owner = None;
    }
    if state.running && state.owner != Some(owner) {
        return Err("Another discovery owner has an active request".into());
    }
    if state.owner.is_some_and(|active| active != owner) {
        return Err("Another discovery owner has pending requests".into());
    }
    state.owner = Some(owner);
    Ok(())
}

fn enqueue_candidate(
    state: &mut DiscoveryState,
    session: &DiscoverySession,
    candidate: &OemCandidate,
) {
    queue_candidate(
        state,
        session,
        &candidate.candidate_id,
        &candidate.source_blob_sha,
        &candidate.unit,
    );
}

fn queue_candidate(
    state: &mut DiscoveryState,
    session: &DiscoverySession,
    candidate_id: &str,
    source_blob_sha: &str,
    unit: &str,
) -> bool {
    let key = (candidate_id.to_owned(), source_blob_sha.to_owned());
    if !state.seen.insert(key) {
        return false;
    }
    state.queue.push_back(QueuedCandidate {
        session: session.clone(),
        candidate_id: candidate_id.into(),
        source_blob_sha: source_blob_sha.into(),
        unit: unit.into(),
    });
    state.results.insert(
        candidate_id.into(),
        OemDiscoveryResult {
            candidate_id: candidate_id.into(),
            state: OemDiscoveryResultState::Queued,
            outcome: None,
            value: None,
            unit: unit.into(),
            detail: None,
            source_blob_sha: source_blob_sha.into(),
            evidence_saved: false,
        },
    );
    true
}

fn reset_session(state: &mut DiscoveryState, session: Option<DiscoverySession>) {
    if state.session == session {
        return;
    }
    state.session = session;
    state.auto_try_visible = false;
    state.queue.clear();
    state.filtered = None;
    state.seen.clear();
    state.results.clear();
    state.last_error = None;
    if !state.running {
        state.current_candidate_id = None;
        state.owner = None;
    }
    state.revision += 1;
}

fn cancel_pending(state: &mut DiscoveryState) {
    state.auto_try_visible = false;
    state.queue.clear();
    state.filtered = None;
    let completed = state
        .results
        .values()
        .filter(|result| result.state != OemDiscoveryResultState::Queued)
        .map(|result| (result.candidate_id.clone(), result.source_blob_sha.clone()))
        .collect::<HashSet<_>>();
    state.seen.retain(|key| completed.contains(key));
    state
        .results
        .retain(|_, result| result.state != OemDiscoveryResultState::Queued);
}

fn priority_candidate_ids(results: &HashMap<String, OemDiscoveryResult>) -> Vec<String> {
    let mut values = results
        .values()
        .filter_map(|result| {
            result
                .value
                .filter(|value| value.is_finite())
                .map(|_| result.candidate_id.clone())
        })
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn state_for_candidate_outcome(
    outcome: &OemCandidateOutcome,
    value: Option<f64>,
) -> OemDiscoveryResultState {
    match outcome {
        OemCandidateOutcome::Positive if value.is_some_and(f64::is_finite) => {
            OemDiscoveryResultState::Value
        }
        OemCandidateOutcome::Positive => OemDiscoveryResultState::Reply,
        OemCandidateOutcome::NoResponse => OemDiscoveryResultState::NoResponse,
        OemCandidateOutcome::NegativeResponse
        | OemCandidateOutcome::ResponsePending
        | OemCandidateOutcome::TemporarilyUnavailable
        | OemCandidateOutcome::SessionRequired
        | OemCandidateOutcome::SecurityRequired => OemDiscoveryResultState::Negative,
        OemCandidateOutcome::InvalidCandidate => OemDiscoveryResultState::Unsupported,
        OemCandidateOutcome::MalformedResponse | OemCandidateOutcome::WrongResponder => {
            OemDiscoveryResultState::Error
        }
    }
}

fn unsupported_adapter_error(error: &str) -> bool {
    error.contains("does not support read-only research tests")
        || error.contains("does not support OEM candidate reads")
        || error.contains("does not support source-backed OEM")
}

fn add_simulation_note(details: &mut Vec<String>, protocol: &str, hardware_family: &str) {
    if protocol.contains("SIMULATED") || hardware_family.eq_ignore_ascii_case("simulator") {
        details.push("SIMULATED; no vehicle transmission".into());
    }
}

fn response(
    service: &OemDiscoveryService,
    candidate_ids: &[String],
    page: Option<OemCatalogPage>,
) -> OemDiscoveryResponse {
    let state = service
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let status = OemDiscoveryStatus {
        adapter_id: state
            .session
            .as_ref()
            .map(|session| session.adapter_id.clone()),
        connection_generation: state
            .session
            .as_ref()
            .map(|session| session.connection_generation),
        auto_try_visible: state.auto_try_visible,
        running: state.running,
        queued: state.queue.len()
            + state
                .filtered
                .as_ref()
                .map(|filtered| filtered.total.saturating_sub(filtered.offset))
                .unwrap_or_default(),
        completed: state
            .results
            .values()
            .filter(|result| {
                !matches!(
                    result.state,
                    OemDiscoveryResultState::Queued | OemDiscoveryResultState::Running
                )
            })
            .count(),
        with_values: state
            .results
            .values()
            .filter(|result| result.state == OemDiscoveryResultState::Value)
            .count(),
        current_candidate_id: state.current_candidate_id.clone(),
        last_error: state.last_error.clone(),
        revision: state.revision,
    };
    let mut seen = HashSet::new();
    let results = candidate_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .filter_map(|id| state.results.get(id).cloned())
        .take(MAX_VISIBLE_CANDIDATES)
        .collect();
    OemDiscoveryResponse {
        status,
        page,
        results,
    }
}

fn lock_state(
    service: &OemDiscoveryService,
) -> Result<std::sync::MutexGuard<'_, DiscoveryState>, String> {
    service
        .state
        .lock()
        .map_err(|_| "OEM discovery state is unavailable".to_owned())
}

fn start_worker(
    app: AppHandle,
    state: Arc<Mutex<DiscoveryState>>,
    owner: Owner,
) -> Result<(), String> {
    {
        let mut state = state
            .lock()
            .map_err(|_| "OEM discovery state is unavailable".to_owned())?;
        if state.running || (state.queue.is_empty() && state.filtered.is_none()) {
            return Ok(());
        }
        state.running = true;
        state.owner = Some(owner);
        state.revision += 1;
    }
    let worker_state = state.clone();
    let spawn = thread::Builder::new()
        .name("oem-discovery".into())
        .spawn(move || run_worker(app, worker_state, owner));
    if let Err(error) = spawn {
        if let Ok(mut state) = state.lock() {
            state.running = false;
            state.owner = None;
            state.last_error = Some(error.to_string());
            state.revision += 1;
        }
        return Err(format!("Could not start OEM discovery worker: {error}"));
    }
    Ok(())
}

fn run_worker(app: AppHandle, state: Arc<Mutex<DiscoveryState>>, owner: Owner) {
    loop {
        if owner == Owner::Agent && !agent_access_unlocked(&app) {
            if let Ok(mut current) = state.lock() {
                cancel_pending(&mut current);
                current.last_error =
                    Some("Agent Access locked or expired; pending discovery stopped".into());
                current.revision += 1;
            }
        }
        let work = match next_work(&app, &state, owner) {
            Ok(Some(work)) => work,
            Ok(None) | Err(_) => break,
        };
        run_candidate(&app, &state, &work);
    }
}

fn next_work(
    app: &AppHandle,
    state: &Arc<Mutex<DiscoveryState>>,
    owner: Owner,
) -> Result<Option<QueuedCandidate>, String> {
    loop {
        let filtered = {
            let mut current = state
                .lock()
                .map_err(|_| "OEM discovery state is unavailable".to_owned())?;
            if current.owner != Some(owner) {
                current.running = false;
                current.current_candidate_id = None;
                if !current.auto_try_visible {
                    current.owner = None;
                }
                current.revision += 1;
                return Ok(None);
            }
            if let Some(candidate) = current.queue.pop_front() {
                current.current_candidate_id = Some(candidate.candidate_id.clone());
                if let Some(result) = current.results.get_mut(&candidate.candidate_id) {
                    result.state = OemDiscoveryResultState::Running;
                }
                current.revision += 1;
                return Ok(Some(candidate));
            }
            let Some(filtered) = current.filtered.as_mut() else {
                current.running = false;
                current.current_candidate_id = None;
                if !current.auto_try_visible {
                    current.owner = None;
                }
                current.revision += 1;
                return Ok(None);
            };
            if filtered.offset >= filtered.total {
                current.filtered = None;
                continue;
            }
            let page = (
                filtered.filter.clone(),
                filtered.offset,
                filtered.token,
                filtered.session.clone(),
            );
            filtered.offset = filtered.offset.saturating_add(PAGE_SIZE);
            current.revision += 1;
            page
        };
        if owner == Owner::Agent && !agent_access_unlocked(app) {
            continue;
        }
        let (filter, offset, token, session) = filtered;
        let page = match catalog(app).and_then(|catalog| {
            catalog.search(
                &filter.query,
                &filter.brand,
                &filter.model,
                filter.service.as_deref(),
                offset,
                PAGE_SIZE,
            )
        }) {
            Ok(page) => page,
            Err(error) => {
                if let Ok(mut current) = state.lock() {
                    if current
                        .filtered
                        .as_ref()
                        .is_some_and(|run| run.token == token)
                    {
                        current.filtered = None;
                        current.last_error = Some(error);
                        current.revision += 1;
                    }
                }
                continue;
            }
        };
        let mut current = state
            .lock()
            .map_err(|_| "OEM discovery state is unavailable".to_owned())?;
        if current
            .filtered
            .as_ref()
            .is_none_or(|run| run.token != token)
            || current.session.as_ref() != Some(&session)
        {
            continue;
        }
        for candidate in &page.candidates {
            enqueue_candidate(&mut current, &session, candidate);
        }
        if page.candidates.is_empty() {
            current.filtered = None;
        }
        current.revision += 1;
    }
}

fn run_candidate(app: &AppHandle, state: &Arc<Mutex<DiscoveryState>>, work: &QueuedCandidate) {
    if let Some(owner) = state.lock().ok().and_then(|state| state.owner)
        && owner == Owner::Agent
        && !agent_access_unlocked(app)
    {
        finish_result(
            state,
            work,
            OemDiscoveryResult {
                candidate_id: work.candidate_id.clone(),
                state: OemDiscoveryResultState::Error,
                outcome: None,
                value: None,
                unit: work.unit.clone(),
                detail: Some(
                    "Agent Access locked or expired; discovery request was not sent".into(),
                ),
                source_blob_sha: work.source_blob_sha.clone(),
                evidence_saved: false,
            },
        );
        return;
    }
    let app_state = app.state::<AppState>();
    let candidate =
        match OemCatalog::new(&app_state.pid_catalog_path).candidate_for_try(&work.candidate_id) {
            Ok(spec) if spec.candidate.source_blob_sha == work.source_blob_sha => spec,
            Ok(_) => {
                finish_unsupported(
                    state,
                    work,
                    "OEM candidate source revision changed before the request".into(),
                );
                return;
            }
            Err(error) => {
                finish_unsupported(state, work, error);
                return;
            }
        };
    if state
        .lock()
        .is_ok_and(|state| state.owner == Some(Owner::Agent))
        && !agent_access_unlocked(app)
    {
        finish_result(
            state,
            work,
            OemDiscoveryResult {
                candidate_id: work.candidate_id.clone(),
                state: OemDiscoveryResultState::Error,
                outcome: None,
                value: None,
                unit: candidate.candidate.unit,
                detail: Some(
                    "Agent Access locked or expired; discovery request was not sent".into(),
                ),
                source_blob_sha: work.source_blob_sha.clone(),
                evidence_saved: false,
            },
        );
        return;
    }
    let result = execute_candidate_try_bound(
        &app_state,
        &work.candidate_id,
        &work.source_blob_sha,
        &format!("discovery-{}", work.session.connection_generation),
        1,
        1,
        &work.session.adapter_id,
        work.session.connection_generation,
    );
    match result {
        Ok(result) => {
            let outcome = serde_json::to_value(&result.outcome)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned));
            let value = result
                .decoded_value
                .as_ref()
                .and_then(|decoded| decoded.value)
                .filter(|value| value.is_finite());
            let mut details = [
                result.error.clone(),
                result.decode_error.clone(),
                result.evidence_save_error.clone(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            add_simulation_note(
                &mut details,
                &result.protocol,
                &result.adapter.hardware_family,
            );
            if result
                .decoded_value
                .as_ref()
                .is_some_and(|decoded| decoded.within_catalog_range == Some(false))
            {
                details.push(
                    "Decoded value is outside the catalog range; treat as research data".into(),
                );
            }
            let discovery_result = OemDiscoveryResult {
                candidate_id: work.candidate_id.clone(),
                state: state_for_candidate_outcome(&result.outcome, value),
                outcome,
                value,
                unit: candidate.candidate.unit,
                detail: (!details.is_empty()).then(|| details.join("; ")),
                source_blob_sha: work.source_blob_sha.clone(),
                evidence_saved: result.evidence_saved,
            };
            finish_result(state, work, discovery_result);
        }
        Err(error) => {
            let detail = error.to_string();
            if is_stale_connection_error(&detail) {
                refresh_live_session(
                    app,
                    &OemDiscoveryService {
                        state: state.clone(),
                    },
                );
            }
            let result = OemDiscoveryResult {
                candidate_id: work.candidate_id.clone(),
                state: if unsupported_adapter_error(&detail) {
                    OemDiscoveryResultState::Unsupported
                } else {
                    OemDiscoveryResultState::Error
                },
                outcome: None,
                value: None,
                unit: candidate.candidate.unit,
                detail: Some(detail.clone()),
                source_blob_sha: work.source_blob_sha.clone(),
                evidence_saved: false,
            };
            finish_result(state, work, result);
        }
    }
}

fn finish_unsupported(state: &Arc<Mutex<DiscoveryState>>, work: &QueuedCandidate, detail: String) {
    finish_result(
        state,
        work,
        OemDiscoveryResult {
            candidate_id: work.candidate_id.clone(),
            state: OemDiscoveryResultState::Unsupported,
            outcome: None,
            value: None,
            unit: work.unit.clone(),
            detail: Some(detail),
            source_blob_sha: work.source_blob_sha.clone(),
            evidence_saved: false,
        },
    );
}

fn finish_result(
    state: &Arc<Mutex<DiscoveryState>>,
    work: &QueuedCandidate,
    result: OemDiscoveryResult,
) {
    if let Ok(mut state) = state.lock() {
        if state.session.as_ref() == Some(&work.session) {
            if matches!(result.state, OemDiscoveryResultState::Error) {
                state.last_error = result.detail.clone();
            }
            state.results.insert(work.candidate_id.clone(), result);
            state.current_candidate_id = None;
            state.revision += 1;
        }
    }
}

fn is_stale_connection_error(error: &str) -> bool {
    error.contains("OBD connection changed before the OEM read")
        || error.contains("connected adapter changed before the OEM read")
        || error.contains("No OBD adapter is connected")
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoverySession, DiscoveryState, OemDiscoveryResult, OemDiscoveryResultState,
        cancel_pending, priority_candidate_ids, queue_candidate, reserve_owner, reset_session,
        state_for_candidate_outcome,
    };
    use lognook_core::OemCandidateOutcome;
    use std::collections::HashMap;

    fn session(generation: u64) -> DiscoverySession {
        DiscoverySession {
            adapter_id: "adapter".into(),
            connection_generation: generation,
        }
    }

    fn result(
        candidate_id: &str,
        state: OemDiscoveryResultState,
        value: Option<f64>,
    ) -> OemDiscoveryResult {
        OemDiscoveryResult {
            candidate_id: candidate_id.into(),
            state,
            outcome: None,
            value,
            unit: "rpm".into(),
            detail: None,
            source_blob_sha: "sha-1".into(),
            evidence_saved: false,
        }
    }

    #[test]
    fn candidates_deduplicate_by_id_and_source_revision() {
        let mut state = DiscoveryState::default();
        state.session = Some(session(1));

        assert!(queue_candidate(
            &mut state,
            &session(1),
            "rpm",
            "sha-1",
            "rpm"
        ));
        assert!(!queue_candidate(
            &mut state,
            &session(1),
            "rpm",
            "sha-1",
            "rpm"
        ));
        assert!(queue_candidate(
            &mut state,
            &session(1),
            "rpm",
            "sha-2",
            "rpm"
        ));
        assert_eq!(state.queue.len(), 2);
    }

    #[test]
    fn reconnect_resets_master_queue_and_prior_results() {
        let mut state = DiscoveryState::default();
        state.session = Some(session(1));
        state.auto_try_visible = true;
        state.results.insert(
            "rpm".into(),
            result("rpm", OemDiscoveryResultState::Value, Some(0.0)),
        );
        queue_candidate(&mut state, &session(1), "next", "sha-1", "rpm");

        reset_session(&mut state, Some(session(2)));

        assert!(!state.auto_try_visible);
        assert!(state.queue.is_empty());
        assert!(state.results.is_empty());
        assert_eq!(state.session, Some(session(2)));
    }

    #[test]
    fn stop_clears_pending_work_but_keeps_completed_results() {
        let mut state = DiscoveryState::default();
        state.session = Some(session(1));
        state.auto_try_visible = true;
        state.results.insert(
            "rpm".into(),
            result("rpm", OemDiscoveryResultState::Value, Some(0.0)),
        );
        queue_candidate(&mut state, &session(1), "next", "sha-1", "rpm");
        state.filtered = Some(super::FilteredRun {
            filter: Default::default(),
            offset: 0,
            total: 100,
            token: 1,
            session: session(1),
        });

        cancel_pending(&mut state);

        assert!(!state.auto_try_visible);
        assert!(state.queue.is_empty());
        assert!(state.filtered.is_none());
        assert_eq!(state.results["rpm"].value, Some(0.0));
    }

    #[test]
    fn value_priority_includes_zero_and_ignores_non_finite_values() {
        let results = HashMap::from([
            (
                "zero".into(),
                result("zero", OemDiscoveryResultState::Value, Some(0.0)),
            ),
            (
                "negative".into(),
                result("negative", OemDiscoveryResultState::Value, Some(-1.0)),
            ),
            (
                "positive".into(),
                result("positive", OemDiscoveryResultState::Value, Some(1.0)),
            ),
            (
                "nan".into(),
                result("nan", OemDiscoveryResultState::Value, Some(f64::NAN)),
            ),
            (
                "reply".into(),
                result("reply", OemDiscoveryResultState::Reply, None),
            ),
        ]);

        assert_eq!(
            priority_candidate_ids(&results),
            ["negative", "positive", "zero"]
        );
    }

    #[test]
    fn zero_is_a_numeric_value_and_non_positive_outcomes_keep_their_state() {
        assert_eq!(
            state_for_candidate_outcome(&OemCandidateOutcome::Positive, Some(0.0)),
            OemDiscoveryResultState::Value
        );
        assert_eq!(
            state_for_candidate_outcome(&OemCandidateOutcome::NoResponse, None),
            OemDiscoveryResultState::NoResponse
        );
        assert_eq!(
            state_for_candidate_outcome(&OemCandidateOutcome::NegativeResponse, None),
            OemDiscoveryResultState::Negative
        );
        assert_eq!(
            state_for_candidate_outcome(&OemCandidateOutcome::MalformedResponse, None),
            OemDiscoveryResultState::Error
        );
    }

    #[test]
    fn auto_try_ownership_prevents_ui_and_agent_runs_from_mixing() {
        let mut state = DiscoveryState {
            auto_try_visible: true,
            owner: Some(super::Owner::Ui),
            ..DiscoveryState::default()
        };

        assert!(reserve_owner(&mut state, super::Owner::Agent).is_err());
        assert!(reserve_owner(&mut state, super::Owner::Ui).is_ok());
    }

    #[test]
    fn request_rejects_unknown_action_and_filter_fields() {
        assert!(
            serde_json::from_str::<super::OemDiscoveryRequest>(
                r#"{"kind":"stop","payload":[{"service":"04"}]}"#
            )
            .is_err()
        );
        assert!(serde_json::from_str::<super::OemDiscoveryRequest>(
            r#"{"kind":"search","filter":{"query":"","brand":"","model":"","service":null,"raw_payload":"04"},"offset":0,"limit":10,"values_first":false}"#
        )
        .is_err());
    }

    #[test]
    fn simulator_results_explicitly_say_no_vehicle_transmission_occurred() {
        let mut details = vec!["positive simulated response".into()];
        super::add_simulation_note(&mut details, "SIMULATED OBD", "simulator");

        assert!(
            details
                .iter()
                .any(|detail| detail == "SIMULATED; no vehicle transmission")
        );
    }
}
