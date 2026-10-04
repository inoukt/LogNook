# Physical enhanced-PID validation workflow

1. Identify the exact vehicle, engine, transmission, ECU/module IDs and adapter.
2. Start with the most specific model/year source; use make fallback only when needed.
3. Generate a plan with `tools/build_validation_plan.py`.
4. For ordinary signal validation, test only the exact source-backed read request and addressing under the standing project authorization. Keep the result as a candidate until applicability and decoding are verified.
5. If a procedure requires session/security access or any state-changing action (including clear, reset, write, erase, routine, or actuator control), stop and obtain explicit approval for that exact test before transmission. Never perform such actions from normal acquisition or automatic polling. Flashing/programming is prohibited.
6. Record the raw response before decoding.
7. Record negative/no-response/wrong-length outcomes; never discard them.
8. Check decoded values for physical plausibility at rest and during a controlled change.
9. Compare with an independent measurement or known standard where practical.
10. Store evidence in `validation_sessions` and `validation_results`.
11. Promote to `PHYSICALLY_VERIFIED` only after the request, byte layout, formula and applicability are confirmed.

## Promotion rule

A candidate may be marked physically verified for a specific vehicle/ECU configuration. That does not make it a universal brand PID.

## Scion xB initial order

ECM/PCM -> TCM -> ABS/VSC -> TPMS -> cluster/body. Prioritize knock, fuel trims, VVT, oil temperature/pressure, misfires, fuel pressure where applicable, transmission temperature, wheel speeds, steering/yaw/brake data.

## Ford Flex initial order

Ford fallback -> related Taurus research -> Explorer -> F-150. Keep related-model evidence separate. Prioritize boost/air path, spark/knock, lambda/fuel pressures, VCT, misfires, transmission, PTU/AWD and chassis signals.
