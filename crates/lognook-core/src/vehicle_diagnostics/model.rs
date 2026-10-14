use super::{decode_payload, empty_read, is_bitmap_page, is_functional_responder};
use crate::{DeviceDescriptor, VehicleIdentitySnapshot};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const MAX_MODE06_TEST_IDS: usize = 32;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DiagnosticRequest {
    #[serde(default)]
    pub mode06_test_ids: Vec<u8>,
}

impl DiagnosticRequest {
    /// The bounded identity-only reads used when matching a saved vehicle profile.
    /// This deliberately excludes DTC and Mode 06 queries from the full diagnostic snapshot.
    pub fn identity_queries() -> VecDeque<DiagnosticQueryRequest> {
        [
            (DiagnosticReadKind::Vin, 0x02),
            (DiagnosticReadKind::CalibrationId, 0x04),
            (DiagnosticReadKind::EcuName, 0x0A),
        ]
        .into_iter()
        .map(|(kind, pid)| DiagnosticQueryRequest {
            kind,
            service: 0x09,
            identifier: Some(pid),
            payload: vec![0x09, pid],
            wait_ms: 1_500,
        })
        .collect()
    }

    pub fn validate(&mut self) -> Result<(), String> {
        if self.mode06_test_ids.len() > MAX_MODE06_TEST_IDS {
            return Err(format!(
                "Select no more than {MAX_MODE06_TEST_IDS} Mode 06 test IDs"
            ));
        }
        if self.mode06_test_ids.iter().any(|id| is_bitmap_page(*id)) {
            return Err("Mode 06 bitmap page IDs are read separately from monitor test IDs".into());
        }
        self.mode06_test_ids.sort_unstable();
        self.mode06_test_ids.dedup();
        Ok(())
    }

    pub fn queries(&self) -> Result<VecDeque<DiagnosticQueryRequest>, String> {
        let mut request = self.clone();
        request.validate()?;
        let queries = [
            (DiagnosticReadKind::StoredDtcs, 0x03, None, vec![0x03], 900),
            (DiagnosticReadKind::PendingDtcs, 0x07, None, vec![0x07], 900),
            (
                DiagnosticReadKind::PermanentDtcs,
                0x0A,
                None,
                vec![0x0A],
                900,
            ),
            (
                DiagnosticReadKind::Vin,
                0x09,
                Some(0x02),
                vec![0x09, 0x02],
                1_500,
            ),
            (
                DiagnosticReadKind::CalibrationId,
                0x09,
                Some(0x04),
                vec![0x09, 0x04],
                1_500,
            ),
            (
                DiagnosticReadKind::EcuName,
                0x09,
                Some(0x0A),
                vec![0x09, 0x0A],
                1_500,
            ),
            (
                DiagnosticReadKind::Mode06Bitmap,
                0x06,
                Some(0x00),
                vec![0x06, 0x00],
                600,
            ),
        ]
        .into_iter()
        .map(
            |(kind, service, identifier, payload, wait_ms)| DiagnosticQueryRequest {
                kind,
                service,
                identifier,
                payload,
                wait_ms,
            },
        )
        .chain(
            request
                .mode06_test_ids
                .into_iter()
                .map(|test_id| DiagnosticQueryRequest {
                    kind: DiagnosticReadKind::Mode06Monitor,
                    service: 0x06,
                    identifier: Some(test_id),
                    payload: vec![0x06, test_id],
                    wait_ms: 600,
                }),
        )
        .collect();
        Ok(queries)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticReadKind {
    StoredDtcs,
    PendingDtcs,
    PermanentDtcs,
    Vin,
    CalibrationId,
    EcuName,
    Mode06Bitmap,
    Mode06Monitor,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticOutcome {
    Positive,
    NegativeResponse,
    ResponsePending,
    TemporarilyUnavailable,
    SessionRequired,
    SecurityRequired,
    NoResponse,
    MalformedResponse,
    WrongResponder,
    TransportError,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum DiagnosticValue {
    DtcList {
        codes: Vec<String>,
    },
    Identity {
        pid: u8,
        label: String,
        value: String,
    },
    Mode06Bitmap {
        start: u8,
        test_ids: Vec<u8>,
        has_more: bool,
    },
    Mode06Monitor {
        test_id: u8,
        component_id: u8,
        uasid: u8,
        value: u16,
        minimum: u16,
        maximum: u16,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiagnosticRead {
    pub kind: DiagnosticReadKind,
    pub service: u8,
    pub identifier: Option<u8>,
    pub request_payload: Vec<u8>,
    pub route: String,
    pub exact_adapter_command: String,
    pub adapter_started_at_unix_ms: u64,
    pub latency_ms: u64,
    pub ecu_id: Option<String>,
    pub outcome: DiagnosticOutcome,
    pub negative_response_code: Option<u8>,
    pub response_payload: Vec<u8>,
    pub values: Vec<DiagnosticValue>,
    pub detail: Option<String>,
    pub raw_response: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiagnosticReport {
    #[serde(default)]
    pub report_id: Option<String>,
    #[serde(default)]
    pub connection_generation: Option<u64>,
    #[serde(default)]
    pub vehicle_context: Option<VehicleIdentitySnapshot>,
    pub adapter: DeviceDescriptor,
    pub protocol: String,
    pub started_at_unix_ms: u64,
    pub request: DiagnosticRequest,
    pub reads: Vec<DiagnosticRead>,
    pub route_restore_error: Option<String>,
    pub evidence_saved: bool,
    pub evidence_save_error: Option<String>,
}

pub fn supports_vehicle_diagnostics(adapter_family: Option<&str>) -> bool {
    matches!(
        adapter_family,
        Some("Xtool A30/A30M Bluetooth SPP") | Some("ELM327 Bluetooth SPP") | Some("OBDLink SX")
    )
}

pub fn supports_profile_identity(adapter_family: Option<&str>) -> bool {
    supports_vehicle_diagnostics(adapter_family) || matches!(adapter_family, Some("OpenPort 2.0"))
}

#[derive(Clone, Debug)]
pub struct DiagnosticQueryRequest {
    pub(crate) kind: DiagnosticReadKind,
    pub(crate) service: u8,
    pub(crate) identifier: Option<u8>,
    pub(crate) payload: Vec<u8>,
    pub(crate) wait_ms: u64,
}

impl DiagnosticQueryRequest {
    pub fn request_payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn validate(&self) -> Result<(), String> {
        let expected = match self.kind {
            DiagnosticReadKind::StoredDtcs => (0x03, None, &[0x03][..]),
            DiagnosticReadKind::PendingDtcs => (0x07, None, &[0x07][..]),
            DiagnosticReadKind::PermanentDtcs => (0x0A, None, &[0x0A][..]),
            DiagnosticReadKind::Vin => (0x09, Some(0x02), &[0x09, 0x02][..]),
            DiagnosticReadKind::CalibrationId => (0x09, Some(0x04), &[0x09, 0x04][..]),
            DiagnosticReadKind::EcuName => (0x09, Some(0x0A), &[0x09, 0x0A][..]),
            DiagnosticReadKind::Mode06Bitmap if self.identifier.is_some_and(is_bitmap_page) => {
                let id = self.identifier.unwrap();
                (0x06, Some(id), &[0x06, id][..])
            }
            DiagnosticReadKind::Mode06Monitor
                if self.identifier.is_some_and(|id| !is_bitmap_page(id)) =>
            {
                let id = self.identifier.unwrap();
                (0x06, Some(id), &[0x06, id][..])
            }
            _ => return Err("Invalid vehicle diagnostic query".into()),
        };
        if self.service != expected.0 || self.identifier != expected.1 || self.payload != expected.2
        {
            return Err(
                "Vehicle diagnostic query does not match its service and identifier".into(),
            );
        }
        Ok(())
    }

    pub fn route(&self) -> &'static str {
        "7DF (functional)"
    }

    pub fn next_mode06_bitmap(&self, reads: &[DiagnosticRead]) -> Option<Self> {
        self.validate().ok()?;
        let start = self.identifier?;
        let has_more = self.kind == DiagnosticReadKind::Mode06Bitmap
            && reads.iter().any(|read| {
                read.outcome == DiagnosticOutcome::Positive
                    && read.values.iter().any(|value| {
                        matches!(value, DiagnosticValue::Mode06Bitmap { has_more: true, .. })
                    })
            });
        let next = start.checked_add(0x20)?;
        has_more
            .then_some(Self {
                kind: DiagnosticReadKind::Mode06Bitmap,
                service: 0x06,
                identifier: Some(next),
                payload: vec![0x06, next],
                wait_ms: 600,
            })
            .filter(|query| query.validate().is_ok())
    }

    pub fn report(
        &self,
        ecu_id: Option<String>,
        payload: Vec<u8>,
        raw: String,
        command: String,
        started_at_unix_ms: u64,
        latency_ms: u64,
        truncated: bool,
    ) -> DiagnosticRead {
        let mut read = empty_read(self, ecu_id, raw, command, started_at_unix_ms, latency_ms);
        read.response_payload = payload;
        if read
            .ecu_id
            .as_deref()
            .is_some_and(|id| !is_functional_responder(id))
        {
            read.outcome = DiagnosticOutcome::WrongResponder;
            read.detail = Some(
                "Functional 11-bit request on 7DF received a response from an unexpected CAN ID"
                    .into(),
            );
        } else if truncated {
            read.outcome = DiagnosticOutcome::MalformedResponse;
            read.detail = Some("Adapter returned a truncated CAN/ISO-TP response".into());
        } else {
            decode_payload(self, &mut read);
        }
        read
    }

    pub fn no_response(
        &self,
        raw: String,
        command: String,
        started_at_unix_ms: u64,
        latency_ms: u64,
        detail: Option<String>,
    ) -> DiagnosticRead {
        let mut read = empty_read(self, None, raw, command, started_at_unix_ms, latency_ms);
        read.outcome = DiagnosticOutcome::NoResponse;
        read.detail = detail;
        read
    }

    pub fn transport_error(
        &self,
        raw: String,
        command: String,
        started_at_unix_ms: u64,
        latency_ms: u64,
        detail: String,
    ) -> DiagnosticRead {
        let mut read = empty_read(self, None, raw, command, started_at_unix_ms, latency_ms);
        read.outcome = DiagnosticOutcome::TransportError;
        read.detail = Some(detail);
        read
    }
}

#[cfg(test)]
mod tests {
    use super::{supports_profile_identity, supports_vehicle_diagnostics};

    #[test]
    fn profile_identity_capability_includes_openport_without_full_diagnostics() {
        for family in [
            "Xtool A30/A30M Bluetooth SPP",
            "ELM327 Bluetooth SPP",
            "OBDLink SX",
            "OpenPort 2.0",
        ] {
            assert!(supports_profile_identity(Some(family)), "{family}");
        }
        assert!(!supports_profile_identity(None));
        assert!(!supports_profile_identity(Some("unknown adapter")));
        assert!(!supports_vehicle_diagnostics(Some("OpenPort 2.0")));
        assert!(supports_vehicle_diagnostics(Some("OBDLink SX")));
    }

    #[test]
    fn report_capture_metadata_round_trips_while_legacy_metadata_defaults() {
        let base = serde_json::json!({
            "adapter": {
                "stable_id": "fixture",
                "hardware_family": "OBDLink SX",
                "firmware": null,
                "path": null
            },
            "protocol": "fixture",
            "started_at_unix_ms": 10,
            "request": { "mode06_test_ids": [] },
            "reads": [],
            "route_restore_error": null,
            "evidence_saved": false,
            "evidence_save_error": null
        });
        let legacy: super::DiagnosticReport = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(legacy.report_id, None);
        assert_eq!(legacy.connection_generation, None);
        assert_eq!(legacy.vehicle_context, None);

        let mut captured = base;
        captured["report_id"] = serde_json::json!("report-fixture-1");
        captured["connection_generation"] = serde_json::json!(7);
        let captured: super::DiagnosticReport = serde_json::from_value(captured).unwrap();
        let saved = serde_json::to_value(captured).unwrap();
        assert_eq!(saved["report_id"], "report-fixture-1");
        assert_eq!(saved["connection_generation"], 7);
        assert!(saved["vehicle_context"].is_null());
    }
}
