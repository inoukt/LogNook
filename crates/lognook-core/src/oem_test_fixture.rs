pub(crate) fn candidate_spec() -> crate::OemCandidateTrySpec {
    crate::OemCandidateTrySpec {
        value_format: "uint".into(),
        source_policy: crate::OemCandidateSourcePolicy::IndexedPack,
        candidate: serde_json::from_value(serde_json::json!({
            "candidate_id":"fixture-temperature", "source_id":"fixture-source",
            "family":"fixture", "brand":"fixture", "model_scope":"fixture",
            "service":"21", "identifier":"01", "request_header":"700", "response_header":"708",
            "frequency_hz":null, "module_type":"Engine", "category":"temperature", "name":"Fixture temperature",
            "signal_source_id":"fixture-signal", "bit_offset":0, "bit_length":8,
            "formula":"raw", "decoder_json":"{\"bix\":0,\"len\":8}", "unit":"celsius",
            "year_filter_json":"{}", "ecu_filter":"REQUIRES_INDIVIDUAL_ECU_VERIFICATION",
            "source_blob_sha":"fixture-revision", "research_state":"community_reference",
            "vehicle_validation":"not_tested", "auto_request":false, "confidence":"research",
            "notes":"Synthetic transport fixture; not a vehicle definition",
            "suggested_metric":"temperature", "source_scope":"fixture", "applicability_specificity":0,
            "min_value":null, "max_value":null, "source_url":"https://example.test/source",
            "license":"fixture", "license_url":"https://example.test/license"
        })).unwrap(),
    }
}
