#[cfg(test)]
mod tests {
    use crate::{
        ConnectionCapabilities, DeviceDescriptor, HardwareAdapter, LogNookError,
        OemCandidateOutcome, OemCandidateTryResult, Sample, SignalDefinition,
    };
    use std::time::Duration;

    struct UnsupportedAdapter;

    impl HardwareAdapter for UnsupportedAdapter {
        fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
            unreachable!()
        }

        fn read_samples(&mut self, _: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
            unreachable!()
        }

        fn sample_interval(&self) -> Duration {
            Duration::from_millis(1)
        }
    }

    #[test]
    fn unsupported_adapter_rejects_research_test() {
        let error = UnsupportedAdapter
            .execute_research_test(crate::ResearchTestRequest::A30ReadTest(
                crate::A30ReadTest::Mode01EngineRpm,
            ))
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not support read-only research tests")
        );
    }

    #[test]
    fn research_result_round_trips_with_tagged_variant() {
        let result = crate::ResearchTestResult::OemCandidate(OemCandidateTryResult {
            candidate_id: "candidate-1".into(),
            exact_adapter_command: "STPX".into(),
            adapter: DeviceDescriptor {
                stable_id: "adapter-1".into(),
                hardware_family: "test".into(),
                firmware: None,
                path: None,
            },
            protocol: "CAN".into(),
            started_at_unix_ms: 1,
            latency_ms: 2,
            outcome: OemCandidateOutcome::Positive,
            raw_response: "621234".into(),
            responder_header: Some("7E8".into()),
            response_payload: vec![0x62, 0x12, 0x34],
            negative_response_code: None,
            decoded_value: None,
            decode_error: None,
            error: None,
            evidence_saved: false,
            evidence_save_error: None,
        });
        let json = serde_json::to_value(&result).unwrap();

        assert_eq!(json["type"], "oemCandidate");
        assert_eq!(json["payload"]["candidate_id"], "candidate-1");
        assert_eq!(
            serde_json::from_value::<crate::ResearchTestResult>(json).unwrap(),
            result
        );
    }
}
use crate::{
    A30ReadTest, A30ReadTestResult, DeviceDescriptor, OemCandidate, OemCandidateTryResult,
    OemCandidateTrySpec,
};
use serde::{Deserialize, Serialize};

pub enum ResearchTestRequest {
    OemCandidate(OemCandidateTrySpec),
    A30ReadTest(A30ReadTest),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum ResearchTestResult {
    OemCandidate(OemCandidateTryResult),
    A30ReadTest(A30ReadTestResult),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum ResearchTestSelection {
    OemCandidate(OemCandidate),
    A30ReadTest(A30ReadTest),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResearchRunItem {
    pub queue_index: usize,
    pub queue_size: usize,
    pub selection: ResearchTestSelection,
    pub result: ResearchTestResult,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResearchRunReport {
    pub run_id: String,
    pub adapter: DeviceDescriptor,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: u64,
    pub items: Vec<ResearchRunItem>,
}
