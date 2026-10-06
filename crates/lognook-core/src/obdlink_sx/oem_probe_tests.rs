use super::{ObdLinkSxAdapter, OemCandidateOutcome, OemCandidateTrySpec};
use crate::{HardwareAdapter, OemCandidate};
use std::{
    io::{Cursor, Read, Write},
    sync::{Arc, Mutex},
};

struct FixturePort {
    input: Cursor<Vec<u8>>,
    written: Arc<Mutex<Vec<u8>>>,
}

impl Read for FixturePort {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.input.read(buffer)
    }
}

impl Write for FixturePort {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.written.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn spec() -> OemCandidateTrySpec {
    OemCandidateTrySpec {
        candidate: OemCandidate {
            candidate_id: "candidate-1".into(),
            source_id: "SCION_SOURCE".into(),
            family: "Toyota".into(),
            brand: "Scion".into(),
            model_scope: "xB 2014".into(),
            service: "21".into(),
            identifier: "01".into(),
            request_header: "7E0".into(),
            response_header: "7E8".into(),
            frequency_hz: None,
            module_type: "Engine".into(),
            category: "Engine".into(),
            name: "Test signal".into(),
            signal_source_id: "SCION_SOURCE_01".into(),
            bit_offset: Some(0),
            bit_length: Some(8),
            formula: "(raw*(1)/(1))+(0)".into(),
            decoder_json: "{\"bix\":0,\"len\":8,\"unit\":\"celsius\"}".into(),
            unit: "celsius".into(),
            year_filter_json: "{}".into(),
            ecu_filter: String::new(),
            source_blob_sha: "reviewed-sha".into(),
            research_state: "community_reference".into(),
            vehicle_validation: "not_tested".into(),
            auto_request: false,
            confidence: "research".into(),
            notes: String::new(),
            suggested_metric: String::new(),
            source_scope: "vehicle".into(),
            applicability_specificity: 60,
            min_value: None,
            max_value: None,
            source_url: Some("https://source.test/vehicle".into()),
            license: Some("CC BY-SA 4.0".into()),
            license_url: Some("https://source.test/license".into()),
        },
        value_format: "uint".into(),
    }
}

#[test]
fn sx_one_shot_sends_one_catalog_command_and_restores_adapter_settings() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let port = FixturePort {
        input: Cursor::new(format!(
            "{}STPRS\rISO 15765-4 (CAN 11/500) [AT]\r\r>OK\r\r>OK\r\r>OK\r\r>7E8 03 61 01 64 00 00 00 00\r\r>OK\r\r>OK\r\r>OK\r\r>",
            include_str!("../../tests/fixtures/obdlink_sx/physical_ford_identity.txt"),
        ).into_bytes()),
        written: written.clone(),
    };
    let mut adapter = ObdLinkSxAdapter::with_port("COM4", Box::new(port));
    let spec = spec();
    let result = adapter.try_oem_candidate_inner(&spec).unwrap();
    adapter.disconnect();

    assert_eq!(result.outcome, OemCandidateOutcome::Positive);
    assert_eq!(result.decoded_value.unwrap().raw_value, "100");
    assert_eq!(
        &*written.lock().unwrap(),
        b"STI\rSTDI\rSTSN\rSTPRS\rATE0\rAT H1\rAT SH 7DF\rSTPX H:7E0, D:2101, T:250\rAT H0\rAT SH 7DF\rATE1\r"
    );
}

#[test]
fn sx_candidate_reuses_connected_session() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let port = FixturePort {
        input: Cursor::new(format!(
            "{}4100 00 10 00 00\r>ISO 15765-4 (CAN 11/500)\r>OK\r>OK\r>OK\r>7E8 06 41 00 00 10 00 00\r>7E8 03 61 01 64\r>7E8 04 62 12 34 64\r>",
            include_str!("../../tests/fixtures/obdlink_sx/physical_ford_identity.txt"),
        ).into_bytes()),
        written: written.clone(),
    };
    let mut adapter = ObdLinkSxAdapter::with_port("no-second-open", Box::new(port));
    let capabilities = adapter.connect().unwrap();
    written.lock().unwrap().clear();

    let crate::ResearchTestResult::OemCandidate(result) = adapter
        .execute_research_test(crate::ResearchTestRequest::OemCandidate(spec()))
        .unwrap()
    else {
        panic!("expected candidate result")
    };

    assert_eq!(result.adapter, capabilities.device);
    assert_eq!(result.outcome, OemCandidateOutcome::Positive);
    assert_eq!(&*written.lock().unwrap(), b"STPX H:7E0, D:2101, T:250\r");

    let mut did = spec();
    did.candidate.service = "22".into();
    did.candidate.identifier = "1234".into();
    let crate::ResearchTestResult::OemCandidate(result) = adapter
        .execute_research_test(crate::ResearchTestRequest::OemCandidate(did))
        .unwrap()
    else {
        panic!("expected candidate result")
    };
    assert_eq!(result.response_payload, [0x62, 0x12, 0x34, 0x64]);
    assert_eq!(
        &*written.lock().unwrap(),
        b"STPX H:7E0, D:2101, T:250\rSTPX H:7E0, D:221234, T:250\r"
    );

    written.lock().unwrap().clear();
    let mut invalid = spec();
    invalid.candidate.request_header = "unverified".into();
    assert!(
        adapter
            .execute_research_test(crate::ResearchTestRequest::OemCandidate(invalid))
            .is_err()
    );
    assert!(
        adapter
            .execute_research_test(crate::ResearchTestRequest::A30ReadTest(
                crate::A30ReadTest::Mode03StoredDtcs
            ))
            .is_err()
    );
    assert!(written.lock().unwrap().is_empty());
}
