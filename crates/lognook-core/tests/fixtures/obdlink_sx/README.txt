Most fixtures are synthetic examples based on the OBDLink FRPM response format.
Their firmware, serial number, and signal values are placeholders.
mode01_supported_20.txt is a synthetic continuation-range reply. The physical Ford
capture establishes that 0100 advertises another range, but no physical 0120 reply
was captured in the reported session.

physical_ford_identity.txt and physical_ford_supported.txt are actual receive bytes
captured 2026-10-02 from OBDLink SX r4.3.1, STN1130 v5.10.1, serial 113011166292,
on a 2011 Ford Flex EcoBoost. Both VCP COM4 and FTDI D2XX returned these responses
at 115200 baud, 8-N-1, no flow control. Original carriage returns, command echo,
spaces, blank lines, and response prompts are preserved. Identity replies are
concatenated in the observed query order STI, STDI, STSN; the other fixture is 0100.
No startup banner preceded the first query. See docs/hardware/2026-10-02-sx-ford.md
for the test record and limits. These captures contain no VIN or DTC information.
