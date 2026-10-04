"""Parse OBDb v3 JSON into inert LogNook OEM candidate rows.

Only read-only data services 0x21 and 0x22 are imported. Imported definitions are
research candidates: auto_request is always 0 until physical validation promotes them.
"""
from __future__ import annotations
import hashlib, json, re
from datetime import date

READ_SERVICES = {"21", "22"}

def git_blob_sha(data: bytes) -> str:
    return hashlib.sha1(f"blob {len(data)}\0".encode("ascii") + data).hexdigest()

def _slug(value: str, limit: int = 96) -> str:
    return re.sub(r"[^A-Z0-9]+", "_", str(value).upper()).strip("_")[:limit]

def _formula(fmt: dict) -> tuple[str, str]:
    if "map" in fmt:
        return "", "enum"
    mul, div, add = fmt.get("mul", 1), fmt.get("div", 1), fmt.get("add", 0)
    if not isinstance(div, (int, float)) or div == 0:
        return "", "unknown"
    sign = bool(fmt.get("sign"))
    return f"(raw*({mul})/({div}))+({add})", ("signed" if sign else "uint")

def _module_map(payload: dict) -> dict[str, str]:
    out = {}
    for ecu in payload.get("ecu", []) or []:
        if isinstance(ecu, dict) and ecu.get("hdr"):
            out[str(ecu["hdr"]).upper()] = str(ecu.get("type") or "")
    return out

def _specificity(source: dict, command: dict) -> int:
    score = 10 if source.get("scope") == "make_fallback" else 40
    if source.get("model"): score += 20
    f = command.get("filter") or {}
    if isinstance(f, dict) and any(k in f for k in ("from", "to", "years")): score += 15
    return min(score, 100)

def parse_payload(source: dict, payload: dict, blob_sha: str) -> list[dict]:
    rows: list[dict] = []
    family, brand = source["family"], source["brand"]
    model_scope, source_id = source.get("model", ""), source["source_id"]
    modules = _module_map(payload)

    for ci, command in enumerate(payload.get("commands", []) or []):
        requests = command.get("cmd", {})
        if not isinstance(requests, dict): continue
        hdr = str(command.get("hdr", "")).upper()
        module_type = modules.get(hdr, "")
        for service, identifier in requests.items():
            service, identifier = str(service).upper(), str(identifier).upper()
            if service not in READ_SERVICES: continue
            if not re.fullmatch(r"[0-9A-F]{2}", service): continue
            if not re.fullmatch(r"[0-9A-F]+", identifier): continue

            for si, signal in enumerate(command.get("signals", []) or []):
                if not isinstance(signal, dict): continue
                fmt = signal.get("fmt") or {}
                bit_length = fmt.get("len")
                if not isinstance(bit_length, int) or not (1 <= bit_length <= 4096): continue
                raw_id = str(signal.get("id") or f"SIGNAL_{ci}_{si}")
                formula, value_format = _formula(fmt)
                decoder = {k: fmt[k] for k in ("map", "sign", "mul", "div", "add", "bix", "len", "unit", "min", "max") if k in fmt}
                response_header = str(command.get("rax", "")).upper()
                identity = {
                    "source_id": source_id, "source_blob_sha": blob_sha,
                    "service": service, "identifier": identifier,
                    "request_header": hdr, "response_header": response_header,
                    "extended_address": command.get("eax"),
                    "filter": command.get("filter"), "dbgfilter": command.get("dbgfilter"),
                    "signal_source_id": raw_id, "signal_index": si,
                }
                identity_sha = hashlib.sha256(json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()).hexdigest()[:12]
                rows.append({
                    "candidate_id": f"{source_id}_{service}{identifier}_{_slug(hdr)}_{_slug(response_header)}_{_slug(raw_id)}_{identity_sha}",
                    "source_id": source_id, "family": family, "brand": brand,
                    "model_scope": model_scope, "service": service, "identifier": identifier,
                    "request_header": hdr, "response_header": response_header,
                    "frequency_hz": command.get("freq"), "category": str(signal.get("path", "OEM")),
                    "name": str(signal.get("name", raw_id)), "signal_source_id": raw_id,
                    "bit_offset": int(fmt.get("bix", 0) or 0), "bit_length": bit_length,
                    "value_format": value_format, "formula": formula,
                    "unit": str(fmt.get("unit", "unknown")),
                    "year_filter_json": json.dumps({"filter": command.get("filter"), "dbgfilter": command.get("dbgfilter"), "config_file": source.get("config_file")}, sort_keys=True, separators=(",", ":")),
                    "ecu_filter": "REQUIRES_INDIVIDUAL_ECU_VERIFICATION",
                    "source_payload": json.dumps({"command": {k:v for k,v in command.items() if k != "signals"}, "signal": signal}, separators=(",", ":")),
                    "source_blob_sha": blob_sha, "research_state": "community_reference",
                    "vehicle_validation": "not_tested", "auto_request": 0, "imported_at": date.today().isoformat(),
                    "module_type": module_type, "suggested_metric": str(signal.get("suggestedMetric", "") or ""),
                    "decoder_json": json.dumps(decoder, sort_keys=True, separators=(",", ":")),
                    "min_value": fmt.get("min") if isinstance(fmt.get("min"), (int,float)) else None,
                    "max_value": fmt.get("max") if isinstance(fmt.get("max"), (int,float)) else None,
                    "source_scope": str(source.get("scope", "")), "applicability_specificity": _specificity(source, command),
                    "confidence": "research", "notes": "",
                })
    return rows
