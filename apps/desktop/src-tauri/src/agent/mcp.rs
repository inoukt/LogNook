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

    #[tool(description = "Search standard OBD PIDs by name, ID, unit, or description.")]
    async fn list_signals(
        &self,
        Parameters(AgentQuery { query }): Parameters<AgentQuery>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ListSignals { query }).await
    }

    #[tool(
        description = "List bounded live PID/CAN buffers and saved LogNook or agent CAN capture sources. CAN ID replay can use only the current live:can buffer; saved captures are read-only."
    )]
    async fn list_log_sources(&self) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ListLogSources).await
    }

    #[tool(
        description = "Read a bounded time range from a live or saved PID/CAN source. Use list_log_sources first; inspect live:can with kind=can, elapsed milliseconds, and the CAN-ID filter before replay_can_id. Replay itself uses only matching frames retained from the current CAN connection."
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
        description = "Connect an explicitly selected source using its existing adapter owner. One OBD adapter and one Candlelight adapter can run together. No bench scan starts on connection; OBD initialization performs normal identity/PID discovery."
    )]
    async fn connect_adapter(
        &self,
        Parameters(AgentConnectArgs {
            choice,
            log_signal_ids,
        }): Parameters<AgentConnectArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::ConnectAdapter {
            choice,
            log_signal_ids,
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
        description = "Read DTC categories, VIN/ECU identity and Mode 06 support from the connected SX, ELM327 or A30. Optional byte-sized monitor IDs add result reads. Returns per-ECU timestamped outcomes, raw replies and decoded values; unmapped values remain raw counts. Does not clear codes."
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
        description = "Cancel the active agent CAN scan, frame send, or selected-ID replay and finalize paired CAN/trace evidence. Use this to interrupt replay."
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
        description = "Operate the connected CAN Workbench: capture, mark an event, scan, send a bounded frame pattern, or replay captured RX frames once for one exact CAN ID/format and live elapsed-time range. For replay, use get_status -> list_log_sources -> query_log_range(live:can, kind=can with CAN-ID/time) to inspect frames, then pass that ID, standard/extended format, and range as replay_can_id. Replay uses only the current connection's retained unflagged classic RX frames (up to 400); saved files and caller-supplied frame data cannot be replayed. TX requires an active-mode Candlelight connection and is recorded to paired app-data evidence. stop_operation cancels replay and closes its paired capture."
    )]
    async fn workbench(
        &self,
        Parameters(AgentWorkbenchArgs { operation }): Parameters<AgentWorkbenchArgs>,
    ) -> Result<Json<AgentResponse>, ErrorData> {
        self.request(AgentRequest::Workbench { operation }).await
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
        info.instructions = Some("Use get_status first. For captured CAN ID replay, follow get_status -> list_log_sources -> query_log_range(live:can, kind=can with a CAN-ID/time range) to inspect current-connection RX frames, then workbench replay_can_id with the same ID, 11/29-bit format, and elapsed-ms range. Replay is one pass from retained unflagged classic frames; saved captures and agent-supplied frame data are not replay sources. Use stop_operation to interrupt replay and finalize paired evidence. LogNook owns adapter I/O. Raw responses are research evidence, not validation. Lock Agent Access in LogNook Settings to revoke access.".into());
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
        let names = tools.into_iter().map(|tool| tool.name).collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "connect_adapter",
                "disconnect_adapter",
                "disconnect_source",
                "export_evidence",
                "get_status",
                "get_trace",
                "list_adapters",
                "list_log_sources",
                "list_mode06_tests",
                "list_signals",
                "query_log_range",
                "read_vehicle_diagnostics",
                "search_oem_candidates",
                "send_raw_command",
                "set_signal_selection",
                "start_trace_capture",
                "stop_operation",
                "stop_trace_capture",
                "try_mode06_test",
                "try_oem_candidate",
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
