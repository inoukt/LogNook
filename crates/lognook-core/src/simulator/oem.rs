use super::*;
use crate::{
    OemCandidateTryResult, OemCandidateTrySpec, build_oem_candidate_command,
    decode_oem_candidate_payload,
};

impl SimulatedObdAdapter {
    pub(super) fn read_oem_candidate(
        &self,
        spec: &OemCandidateTrySpec,
    ) -> Result<OemCandidateTryResult, LogNookError> {
        build_oem_candidate_command(spec)?;
        if self.started.is_none() {
            return Err(LogNookError::new("Simulator is not connected"));
        }
        let service = u8::from_str_radix(&spec.candidate.service, 16).unwrap();
        let identifier = spec
            .candidate
            .identifier
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        // Synthetic zero bytes exercise catalog decoding without claiming real ECU support.
        let mut payload = [vec![service + 0x40], identifier].concat();
        let bits = u64::from(spec.candidate.bit_offset.unwrap_or(0))
            + u64::from(spec.candidate.bit_length.unwrap_or(8));
        let bytes = bits.div_ceil(8).clamp(64, 1024) as usize;
        payload.resize(payload.len() + bytes, 0);
        let decoded =
            decode_oem_candidate_payload(spec, Some(&spec.candidate.response_header), &payload);
        Ok(OemCandidateTryResult {
            candidate_id: spec.candidate.candidate_id.clone(),
            exact_adapter_command: format!(
                "SIMULATED {} {} (no vehicle transmission)",
                spec.candidate.service, spec.candidate.identifier
            ),
            adapter: DeviceDescriptor {
                stable_id: "simulated-obd".into(),
                hardware_family: "simulator".into(),
                firmware: Some("LogNook scripted OBD v1".into()),
                path: None,
            },
            protocol: "SIMULATED OEM research (no vehicle transmission)".into(),
            started_at_unix_ms: crate::vehicle_diagnostics::now_unix_ms(),
            latency_ms: 0,
            outcome: decoded.outcome,
            raw_response: format!("SIMULATED {} {:?}", spec.candidate.response_header, payload),
            responder_header: decoded.responder_header,
            response_payload: decoded.payload,
            negative_response_code: decoded.negative_response_code,
            decoded_value: decoded.decoded_value,
            decode_error: decoded.decode_error,
            error: decoded.error,
            evidence_saved: false,
            evidence_save_error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_source_candidate_has_zero_value_and_an_explicit_simulation_label() {
        let spec = crate::oem_test_fixture::candidate_spec();
        let mut adapter = SimulatedObdAdapter::default();
        assert!(adapter.read_oem_candidate(&spec).is_err());
        adapter.connect().unwrap();
        let result = adapter.read_oem_candidate(&spec).unwrap();
        assert_eq!(result.outcome, crate::OemCandidateOutcome::Positive);
        assert_eq!(result.decoded_value.unwrap().value, Some(0.0));
        assert!(result.protocol.contains("SIMULATED"));
        assert!(
            result
                .exact_adapter_command
                .contains("no vehicle transmission")
        );
        assert!(!spec.candidate.auto_request);
        let mut bad = spec;
        bad.candidate.source_url = None;
        assert!(adapter.read_oem_candidate(&bad).is_err());
    }
}
