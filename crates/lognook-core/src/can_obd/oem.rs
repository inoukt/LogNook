use super::*;
use crate::{
    OemCandidateOutcome, OemCandidateTryResult, OemCandidateTrySpec, build_oem_candidate_command,
    decode_oem_candidate_payload, decode_oem_candidate_response,
};

impl CanObdAdapter {
    pub(super) fn read_oem_candidate(
        &mut self,
        spec: &OemCandidateTrySpec,
    ) -> Result<OemCandidateTryResult, LogNookError> {
        build_oem_candidate_command(spec)?;
        if self.started.is_none() {
            return Err(LogNookError::new("Candlelight OBD is not connected"));
        }
        self.ensure_not_cancelled()?;
        let request_id = u32::from_str_radix(&spec.candidate.request_header, 16)
            .map_err(|error| LogNookError::new(error.to_string()))?;
        let response_id = u32::from_str_radix(&spec.candidate.response_header, 16)
            .map_err(|error| LogNookError::new(error.to_string()))?;
        let service = u8::from_str_radix(&spec.candidate.service, 16)
            .map_err(|error| LogNookError::new(error.to_string()))?;
        let identifier = spec
            .candidate
            .identifier
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let payload = [vec![service], identifier.clone()].concat();
        let mut data = vec![0; 8];
        data[0] = payload.len() as u8;
        data[1..1 + payload.len()].copy_from_slice(&payload);
        let started_at_unix_ms = crate::vehicle_diagnostics::now_unix_ms();
        let started = Instant::now();
        let frame = CanFrame {
            elapsed_ns: 0,
            wall_time_unix_ns: 0,
            can_id: request_id,
            extended: false,
            direction: CanDirection::Tx,
            data,
            flags: 0,
        };
        self.transport
            .as_mut()
            .ok_or_else(|| LogNookError::new("Candlelight transport is closed"))?
            .transmit(&frame)
            .map_err(|error| LogNookError::new(error.to_string()))?;
        // This one-shot research request uses the source's exact route. It does
        // not change the Mode 01 ECU or enable continuous OEM acquisition.
        let responses =
            self.receive_routed_payloads(service, &identifier, 1000, request_id, response_id)?;
        let raw_response = responses
            .iter()
            .flat_map(|(frames, _, _)| frames.iter())
            .map(frame_hex)
            .collect::<Vec<_>>()
            .join("\n");
        let mut decoded = if let Some((_, payload, _)) = responses.last() {
            decode_oem_candidate_payload(spec, Some(&spec.candidate.response_header), payload)
        } else {
            decode_oem_candidate_response(spec, "")
        };
        if responses.last().is_some_and(|(_, _, truncated)| *truncated) {
            decoded.outcome = OemCandidateOutcome::MalformedResponse;
            decoded.decoded_value = None;
            decoded.error = Some("Incomplete or sequence-invalid ISO-TP OEM response".into());
        }
        let source = self
            .source_id
            .as_deref()
            .ok_or_else(|| LogNookError::new("Candlelight identity unavailable"))?;
        let channel = self
            .transport
            .as_ref()
            .and_then(|transport| transport.target_channel())
            .ok_or_else(|| LogNookError::new("Candlelight channel unavailable"))?;
        Ok(OemCandidateTryResult {
            candidate_id: spec.candidate.candidate_id.clone(),
            exact_adapter_command: format!(
                "CAN 11-bit TX {request_id:03X} {}",
                hex_bytes(&payload)
            ),
            adapter: DeviceDescriptor {
                stable_id: format!("{source}:channel:{channel}:obd"),
                hardware_family: "Candlelight GS-USB OBD-II".into(),
                firmware: None,
                path: Some(self.selection.id.clone()),
            },
            protocol: "Source-backed OEM over 11-bit CAN at 500 kbit/s".into(),
            started_at_unix_ms,
            latency_ms: started.elapsed().as_millis() as u64,
            outcome: decoded.outcome,
            raw_response,
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
