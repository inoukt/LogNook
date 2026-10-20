use super::{ipc::parse_endpoint, protocol::*};
use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::traits::tokio::Stream as _;
use rmcp::{
    ErrorData, Json, ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream},
    sync::Mutex,
};

const ENDPOINT_ENV: &str = "LOGNOOK_AGENT_SOCKET";

struct IpcClient {
    stream: BufStream<Stream>,
}

impl IpcClient {
    async fn connect(endpoint: &str) -> io::Result<Self> {
        let stream = Stream::connect(parse_endpoint(endpoint)?)
            .await
            .map_err(|error| io::Error::new(io::ErrorKind::NotConnected, error))?;
        Ok(Self {
            stream: BufStream::new(stream),
        })
    }

    async fn request(&mut self, request: &AgentRequest) -> io::Result<AgentResponse> {
        let mut bytes = serde_json::to_vec(request).map_err(io::Error::other)?;
        bytes.push(b'\n');
        self.stream.write_all(&bytes).await?;
        self.stream.flush().await?;
        let mut response = Vec::new();
        let length = (&mut self.stream)
            .take((MAX_IPC_MESSAGE_BYTES + 1) as u64)
            .read_until(b'\n', &mut response)
            .await?;
        if length == 0 || length > MAX_IPC_MESSAGE_BYTES || response.last() != Some(&b'\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Agent Access response",
            ));
        }
        response.pop();
        serde_json::from_slice(&response).map_err(io::Error::other)
    }
}

#[derive(Clone)]
struct LogNookAgent {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    client: Arc<Mutex<IpcClient>>,
}

#[tool_router]
impl LogNookAgent {
    fn new(client: IpcClient) -> Self {
        Self {
            tool_router: Self::tool_router(),
            client: Arc::new(Mutex::new(client)),
        }
    }

    #[tool(
        description = "Read LogNook connection, Agent Access, and current operation status before taking action."
    )]
    async fn get_status(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::GetStatus).await
    }

    #[tool(
        description = "List adapters currently available to LogNook. Use the returned typed choice to connect; discovery does not transmit."
    )]
    async fn list_adapters(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ListAdapters).await
    }

    #[tool(
        description = "Operate normal Candlelight GS-USB OBD logging. discover lists USB candidates without vehicle traffic; identify checks the selected USB interface/channel only; connect discovers generic SAE Mode 01 support and starts selected signals in the ordinary acquisition/recording pipeline; select updates log/graph channels; status reports the active OBD connection; disconnect stops only this OBD source; cancel cancels an in-progress connect. First release uses 11-bit CAN at 500 kbit/s, read-only Mode 01, and one unique standard ECU responder. It does not scan OEM identifiers or run VIN/profile recognition. The same physical USB interface/channel cannot be owned simultaneously by CAN Workbench and OBD logging."
    )]
    async fn can_obd(
        &self,
        Parameters(args): Parameters<AgentCanObdArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::CanObd {
            operation: args.operation,
        })
        .await
    }

    #[tool(description = "Search standard OBD PIDs by name, ID, unit, or description.")]
    async fn list_signals(
        &self,
        Parameters(AgentQuery { query }): Parameters<AgentQuery>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ListSignals { query }).await
    }

    #[tool(
        description = "List bounded live PID/CAN buffers and saved or session-registered LogNook CAN capture sources. Inspect a listed saved CAN source, then prepare_saved_can_replay for an explicit one-ID/time-range transmission preview. Listing never transmits."
    )]
    async fn list_log_sources(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ListLogSources).await
    }

    #[tool(
        description = "Read a bounded time range from a listed live or saved PID/CAN source. Inspect live:can before replay_can_id; inspect a recorded CAN source before prepare_saved_can_replay. Time ranges are source elapsed milliseconds; RX/TX and standard/extended formats remain distinct."
    )]
    async fn query_log_range(
        &self,
        Parameters(query): Parameters<AgentLogQueryArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::QueryLogRange { query }).await
    }

    #[tool(
        description = "Set LogNook's PID log and graph selections. Logging changes are rejected while an adapter is connected; graph changes are immediate."
    )]
    async fn set_signal_selection(
        &self,
        Parameters(AgentSelectionArgs {
            log_signal_ids,
            graph_signal_ids,
        }): Parameters<AgentSelectionArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::SetSignalSelection {
            log_signal_ids,
            graph_signal_ids,
        })
        .await
    }

    #[tool(
        description = "Connect an explicitly selected source through its existing adapter owner. OpenPort choices include normal OBD-II CAN and a dedicated read-only Subaru SSM K-Line mode. Use the latter to graph manually selected SSM-KWP catalog signals; it does not enable standard OBD diagnostics. Set profile_standby=true for supported OBD connections that discover support and wait for vehicle recognition and explicit Resume. For Candlelight GS-USB OBD use can_obd so connect cancellation and status share its owner. Connecting never starts a scan or replay."
    )]
    async fn connect_adapter(
        &self,
        Parameters(AgentConnectArgs {
            choice,
            log_signal_ids,
            profile_standby,
        }): Parameters<AgentConnectArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ConnectAdapter {
            choice,
            log_signal_ids,
            profile_standby,
        })
        .await
    }

    #[tool(
        description = "Disconnect both OBD and CAN adapters and finalize the shared recording. Use disconnect_source to stop only one."
    )]
    async fn disconnect_adapter(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::DisconnectAdapter).await
    }

    #[tool(
        description = "Search the source-backed OEM research catalog. Results keep the ECU route and source metadata; they are not validated or automatically polled."
    )]
    async fn search_oem_candidates(
        &self,
        Parameters(AgentOemSearchArgs {
            query,
            brand,
            model,
            service,
            offset,
            limit,
        }): Parameters<AgentOemSearchArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::SearchOemCandidates {
            query,
            brand,
            model,
            service,
            offset,
            limit,
        })
        .await
    }

    #[tool(
        description = "Search the Subaru OEM catalog by signal, source address, model year, ECU, or protocol. SSM-KWP rows can be explicitly added to live graphs while connected to OpenPort 2 in Subaru SSM K-Line mode; the adapter sends only SSM initialization and memory-read commands. FHI-CAN rows use the CAN Workbench. Source mappings and vehicle/ECU support remain unverified, and normal preset/automatic polling stays disabled."
    )]
    async fn search_subaru_catalog(
        &self,
        Parameters(AgentSubaruCatalogSearchArgs {
            query,
            model,
            year,
            ecu_id,
            protocol_family,
            offset,
            limit,
        }): Parameters<AgentSubaruCatalogSearchArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::SearchSubaruCatalog {
            query,
            model,
            year,
            ecu_id,
            protocol_family,
            offset,
            limit,
        })
        .await
    }

    #[tool(
        description = "Load decoder fields and source-row provenance for one Subaru OEM catalog result. The result is research data and does not establish vehicle support."
    )]
    async fn get_subaru_catalog_detail(
        &self,
        Parameters(AgentSubaruCatalogDetailArgs { candidate_id }): Parameters<
            AgentSubaruCatalogDetailArgs,
        >,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::GetSubaruCatalogDetail { candidate_id })
            .await
    }

    #[tool(
        description = "Run one explicit OEM candidate request on the connected adapter and return typed response/evidence details."
    )]
    async fn try_oem_candidate(
        &self,
        Parameters(AgentTryOemArgs { candidate_id }): Parameters<AgentTryOemArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::TryOemCandidate { candidate_id })
            .await
    }

    #[tool(
        description = "Use the same one-shot OEM discovery queue as the catalog UI. search returns a bounded filtered page and colored-result states; set_auto_try explicitly enables enqueue_visible for caller-listed visible candidate IDs; try_filtered tries the entire filtered list once and can be stopped. Bind starts/enqueues to adapter_id and connection_generation from get_status. status accepts page candidate_ids and returns progress/results; search values_first=true puts finite numeric values, including zero, first across pages. Source-backed services 21/22 only; results do not promote vehicle validation or enable continuous logging. Simulator replies are synthetic. Agent-owned discovery stops on Agent Access lock or connection change."
    )]
    async fn oem_discovery(
        &self,
        Parameters(args): Parameters<AgentOemDiscoveryArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::OemDiscovery {
            operation: args.operation,
        })
        .await
    }

    #[tool(
        description = "Inspect or control continuous OEM logging through the serialized acquisition scheduler. Standard start requires exact-vehicle eligible candidates and the current adapter_id/connection_generation from get_status. StartSubaru accepts candidate IDs returned by search_subaru_catalog and logs only explicitly selected Subaru CAN or SSM-KWP signals with a compatible adapter. For SSM-KWP, connect OpenPort 2 in Subaru SSM K-Line mode; reads keep raw bytes and catalog SHA provenance and remain suspect/unverified. Automatic requests and presets stay disabled. StopSubaru stops only the Subaru plan. No raw payloads, write commands, session changes or validation overrides are accepted. Status reports actual samples, requests, errors and latency."
    )]
    async fn oem_logging(
        &self,
        Parameters(args): Parameters<AgentOemLoggingArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::OemLogging {
            operation: args.operation,
        })
        .await
    }

    #[tool(
        description = "List Mode 06 read tests supported by the connected OBDLink SX or A30 adapter."
    )]
    async fn list_mode06_tests(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ListMode06Tests).await
    }

    #[tool(
        description = "Disconnect only the OBD or CAN source. A connected second source keeps acquiring and recording. disconnect_adapter disconnects both."
    )]
    async fn disconnect_source(
        &self,
        Parameters(AgentDisconnectSourceArgs { source }): Parameters<AgentDisconnectSourceArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::DisconnectSource { source })
            .await
    }

    #[tool(
        description = "Read DTC categories, VIN/ECU identity and Mode 06 support from the connected SX, ELM327 or A30. Optional byte-sized monitor IDs add result reads. Returns per-ECU timestamped outcomes and decoded values; VIN values/raw bytes are masked here and remain in local history. Use the explicit vehicle_profiles read_identity or get_history_report operation when full VIN detail is needed. Unmapped Mode 06 values remain raw counts. Does not clear codes."
    )]
    async fn read_vehicle_diagnostics(
        &self,
        Parameters(AgentVehicleDiagnosticsArgs { mode06_test_ids }): Parameters<
            AgentVehicleDiagnosticsArgs,
        >,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ReadVehicleDiagnostics { mode06_test_ids })
            .await
    }

    #[tool(
        description = "Explicitly send one standard Mode 04 DTC-clear request through the connected adapter. This is separate from read_vehicle_diagnostics and requires Agent Access to be unlocked, confirm=true, and the exact current adapter_id and connection_generation from get_status. The request can clear emission-related stored information and affect readiness. Results record per-ECU acknowledgements and errors; a positive 44 response is only an ECU acknowledgement."
    )]
    async fn clear_vehicle_dtcs(
        &self,
        Parameters(args): Parameters<AgentClearVehicleDtcsArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ClearVehicleDtcs {
            adapter_id: args.adapter_id,
            connection_generation: args.connection_generation,
            confirm: args.confirm,
        })
        .await
    }

    #[tool(
        description = "Run one explicit Mode 06 read test on a connected OBDLink SX or A30. Each result is retained as research evidence."
    )]
    async fn try_mode06_test(
        &self,
        Parameters(AgentMode06TryArgs { test }): Parameters<AgentMode06TryArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::TryMode06Test { test }).await
    }

    #[tool(
        description = "Read adapter traffic from a cursor. Pages are capped at 500 events and report gaps from dropped or evicted entries."
    )]
    async fn get_trace(
        &self,
        Parameters(AgentTraceArgs {
            after_sequence,
            limit,
        }): Parameters<AgentTraceArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::GetTrace {
            after_sequence,
            limit,
        })
        .await
    }

    #[tool(description = "Start an explicit JSONL capture of adapter traffic visible to LogNook.")]
    async fn start_trace_capture(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::StartTraceCapture).await
    }

    #[tool(description = "Stop the current JSONL trace capture and return its path.")]
    async fn stop_trace_capture(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::StopTraceCapture).await
    }

    #[tool(
        description = "Cancel pending replay preparation/profile recognition and the active agent CAN scan, frame send, or live/saved selected-ID replay. Finalize owned paired CAN/trace evidence. Stop uses actual replay state rather than an estimated end time."
    )]
    async fn stop_operation(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::StopOperation).await
    }

    #[tool(
        description = "Copy the latest stopped trace file into the LogNook evidence export directory."
    )]
    async fn export_evidence(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ExportEvidence).await
    }

    #[tool(
        description = "Operate the CAN Workbench: capture, mark, scan, send, or replay captured RX frames once. Live replay_can_id uses the current retained buffer (up to 400). Saved replay: list_log_sources -> query_log_range -> prepare_saved_can_replay -> inspect_saved_can_replay -> explicitly start_saved_can_replay with preview_id, expected_can_generation and the reviewed expected_output_capture_id. Clips contain 1-10000 valid unflagged classic RX frames and at most 600 seconds of source/paced time; limits reject rather than truncate. Source files remain unchanged. TX requires active-mode Candlelight and a separate paired output capture; known source bitrate must match. Submitted frames/adapter echoes do not prove ECU acceptance. Use stop_operation to cancel and finalize evidence. Caller-supplied replay payload arrays are unavailable."
    )]
    async fn workbench(
        &self,
        Parameters(AgentWorkbenchArgs { operation }): Parameters<AgentWorkbenchArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::Workbench { operation }).await
    }

    #[tool(
        description = "Manage local Vehicle/ECU profiles and diagnostic history through the same service as the UI. Read VIN/calibration/ECU identity with read_identity; decode stored VINs offline; add clearly marked Agent Access vehicle details; browse paginated history and verify report evidence; append manual report associations; import older reports with status/cancel. Connect with profile_standby=true, inspect status/generation, recognize identity, then prepare_resume and explicitly apply_resume with the preview revision and connection generation. Resume rechecks fresh supported generic signals; unverified OEM never becomes automatic polling. These operations never clear DTCs or write to the vehicle."
    )]
    async fn vehicle_profiles(
        &self,
        Parameters(AgentVehicleProfileArgs { operation }): Parameters<AgentVehicleProfileArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::VehicleProfiles { operation })
            .await
    }

    #[tool(description = "Manage VIN-keyed vehicle projects. prepare_on_connect uses the current fresh positive VIN; new projects seed supported generic Mode 01 signals and a basic preset. Existing projects require an explicit load_last_state or start_fresh choice. Restores revalidate current signal support and keep OEM research references gated; they never test OEM signals or start a recording. Agent Access must be unlocked.")]
    async fn vehicle_projects(
        &self,
        Parameters(crate::commands::vehicle_projects::VehicleProjectsArgs { operation }): Parameters<crate::commands::vehicle_projects::VehicleProjectsArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::VehicleProjects { operation }).await
    }

    #[tool(description = "Browse, filter, preview, apply, save, edit, delete, stage, or cancel signal presets through LogNook's shared preset service. Apply requires a fresh preview and explicit preview ID plus context revision and connection generation. The operation never promotes an OEM research candidate; unverified channels remain skipped.")]
    async fn signal_presets(
        &self,
        Parameters(AgentSignalPresetsArgs { operation }): Parameters<AgentSignalPresetsArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::SignalPresets { operation }).await
    }

    #[tool(
        description = "Send one raw diagnostic payload (hex bytes) or adapter-terminal command over a connected OBDLink SX or ELM327. The serialized adapter owner records TX/RX in its trace; use workbench send_frame for raw Candlelight CAN."
    )]
    async fn send_raw_command(
        &self,
        Parameters(AgentRawCommandArgs { operation }): Parameters<AgentRawCommandArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::SendRawCommand { operation })
            .await
    }
}

impl LogNookAgent {
    async fn request(&self, request: AgentRequest) -> Result<Json<AgentResponse>, ErrorData> {
        let response = self
            .client
            .lock()
            .await
            .request(&request)
            .await
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        Ok(Json(response))
    }

    async fn app_is_alive(&self) -> bool {
        let Ok(mut client) = self.client.try_lock() else {
            return true;
        };
        client.request(&AgentRequest::GetStatus).await.is_ok()
    }
}

#[tool_handler]
impl ServerHandler for LogNookAgent {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some("Use get_status first. OEM one-shot discovery uses oem_discovery search/status, then explicit set_auto_try plus enqueue_visible or try_filtered bound to the current adapter_id and connection_generation; stop cancels queued work. Numeric values including zero are research evidence, not vehicle validation. Live CAN replay uses list_log_sources -> query_log_range(live:can) -> workbench replay_can_id. Saved CAN replay uses a listed recorded source -> query_log_range -> prepare_saved_can_replay -> inspect_saved_can_replay -> explicit start_saved_can_replay with the preview token and current target generation. Replay transmits one exact ID/format once from backend-frozen RX frames through the existing owner; caller payloads are not a replay source. Vehicle identity and history use the typed vehicle_profiles tool: read_identity performs only Mode 09 VIN/calibration/ECU-name reads; decode_vin_offline is local; report details verify stored bytes and hashes; history associations are append-only. Generic read_vehicle_diagnostics masks Mode 09 PID 02 values and payload bytes and never clears codes; use explicit identity/history detail to request the full VIN. clear_vehicle_dtcs is a separate one-shot Mode 04 action requiring unlocked Agent Access, confirm=true, and the current adapter_id plus obd_connection_generation from get_status. A positive 44 is only an ECU acknowledgement. Full VIN and manually entered details are personal local data. Profile Resume uses fresh support/identity and a connection/revision-bound preview; saved selections never enable unverified OEM polling. Use cancel_history_import to stop an import, cancel/stop_operation for profile and CAN operations. Raw responses are research evidence, not validation. Lock Agent Access in Settings to revoke access.".into());
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}

pub async fn run_agent_mcp() -> Result<(), String> {
    let endpoint = std::env::var(ENDPOINT_ENV).map_err(|_| "LogNook Agent MCP needs the app-provided local endpoint. Open LogNook, unlock Agent Access, and copy the current setup again.".to_owned())?;
    let client = IpcClient::connect(&endpoint).await.map_err(|_| "Could not reach LogNook. Open the app, unlock Agent Access in Settings, then restart the agent connection.".to_owned())?;
    let agent = LogNookAgent::new(client);
    let service = agent
        .clone()
        .serve(stdio())
        .await
        .map_err(|error| error.to_string())?;
    let cancellation = service.cancellation_token();
    let mut monitor = tokio::spawn(wait_for_app_disconnect(agent));
    let result = tokio::select! {
        result = service.waiting() => result.map(|_| ()).map_err(|error| error.to_string()),
        _ = &mut monitor => {
            cancellation.cancel();
            Ok(())
        }
    };
    monitor.abort();
    result
}

async fn wait_for_app_disconnect(agent: LogNookAgent) {
    loop {
        if !agent.app_is_alive().await {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interprocess::local_socket::{ListenerOptions, traits::tokio::Listener as _};

    #[test]
    fn mcp_lists_status_tool_for_stdio_server() {
        let tools = LogNookAgent::tool_router().list_all();
        let workbench = tools.iter().find(|tool| tool.name == "workbench").unwrap();
        assert!(
            workbench
                .description
                .as_deref()
                .is_some_and(|description| description.contains("replay captured RX frames"))
        );
        assert!(
            serde_json::to_string(workbench.input_schema.as_ref())
                .unwrap()
                .contains("replay_can_id")
        );
        let can_obd = tools.iter().find(|tool| tool.name == "can_obd").unwrap();
        assert!(can_obd.description.as_deref().unwrap().contains("Mode 01"));
        let can_obd_schema = serde_json::to_string(can_obd.input_schema.as_ref()).unwrap();
        assert!(can_obd_schema.contains("identify"));
        assert!(can_obd_schema.contains("cancel"));
        let clear = tools
            .iter()
            .find(|tool| tool.name == "clear_vehicle_dtcs")
            .unwrap();
        assert!(
            clear
                .description
                .as_deref()
                .unwrap()
                .contains("confirm=true")
        );
        let clear_schema = serde_json::to_string(clear.input_schema.as_ref()).unwrap();
        assert!(clear_schema.contains("connection_generation"));
        assert!(clear_schema.contains("adapter_id"));
        assert!(clear_schema.contains("confirm"));
        let discovery = tools
            .iter()
            .find(|tool| tool.name == "oem_discovery")
            .unwrap();
        assert!(
            discovery
                .description
                .as_deref()
                .unwrap()
                .contains("including zero")
        );
        let discovery_schema = serde_json::to_string(discovery.input_schema.as_ref()).unwrap();
        for operation in [
            "enqueue_visible",
            "try_filtered",
            "stop",
            "values_first",
            "connection_generation",
        ] {
            assert!(discovery_schema.contains(operation));
        }
        let logging = tools
            .iter()
            .find(|tool| tool.name == "oem_logging")
            .unwrap();
        let schema = serde_json::to_string(logging.input_schema.as_ref()).unwrap();
        for field in [
            "start",
            "stop",
            "candidate_ids",
            "connection_generation",
            "vehicle_id",
        ] {
            assert!(schema.contains(field));
        }
        let connect = tools
            .iter()
            .find(|tool| tool.name == "connect_adapter")
            .unwrap();
        let connect_schema = serde_json::to_string(connect.input_schema.as_ref()).unwrap();
        assert!(connect_schema.contains("open_port2_subaru_ssm"));
        assert!(connect.description.as_deref().unwrap().contains("Subaru SSM K-Line"));
        let subaru = tools
            .iter()
            .find(|tool| tool.name == "search_subaru_catalog")
            .unwrap();
        let subaru_schema = serde_json::to_string(subaru.input_schema.as_ref()).unwrap();
        for field in [
            "query",
            "model",
            "year",
            "ecu_id",
            "protocol_family",
            "offset",
            "limit",
        ] {
            assert!(
                subaru_schema.contains(field),
                "Subaru search schema includes {field}"
            );
        }
        let subaru_detail = tools
            .iter()
            .find(|tool| tool.name == "get_subaru_catalog_detail")
            .unwrap();
        assert!(
            serde_json::to_string(subaru_detail.input_schema.as_ref())
                .unwrap()
                .contains("candidate_id")
        );
        let presets = tools
            .iter()
            .find(|tool| tool.name == "signal_presets")
            .unwrap();
        let presets_schema = serde_json::to_value(presets.input_schema.as_ref()).unwrap();
        assert_eq!(presets_schema.get("type").and_then(|value| value.as_str()), Some("object"));
        assert!(serde_json::to_string(&presets_schema).unwrap().contains("expected_generation"));
        let names = tools.into_iter().map(|tool| tool.name).collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "can_obd",
                "clear_vehicle_dtcs",
                "connect_adapter",
                "disconnect_adapter",
                "disconnect_source",
                "export_evidence",
                "get_status",
                "get_subaru_catalog_detail",
                "get_trace",
                "list_adapters",
                "list_log_sources",
                "list_mode06_tests",
                "list_signals",
                "oem_discovery",
                "oem_logging",
                "query_log_range",
                "read_vehicle_diagnostics",
                "search_oem_candidates",
                "search_subaru_catalog",
                "send_raw_command",
                "set_signal_selection",
                "signal_presets",
                "start_trace_capture",
                "stop_operation",
                "stop_trace_capture",
                "try_mode06_test",
                "try_oem_candidate",
                "vehicle_profiles",
                "workbench",
            ]
        );
    }

    #[tokio::test]
    async fn mcp_detects_when_the_lognook_app_disconnects() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        #[cfg(windows)]
        let endpoint = format!("LogNookMcpDisconnect{suffix}");
        #[cfg(not(windows))]
        let endpoint = std::env::temp_dir()
            .join(format!("lognook-mcp-disconnect-{suffix}.sock"))
            .to_string_lossy()
            .into_owned();
        let listener = ListenerOptions::new()
            .name(parse_endpoint(&endpoint).unwrap())
            .create_tokio()
            .unwrap();
        let server = tokio::spawn(async move {
            drop(listener.accept().await.unwrap());
        });
        let client = IpcClient::connect(&endpoint).await.unwrap();
        server.await.unwrap();
        let agent = LogNookAgent::new(client);

        tokio::time::timeout(Duration::from_secs(1), wait_for_app_disconnect(agent))
            .await
            .expect("MCP sidecar should exit after the LogNook IPC connection closes");
    }
}
