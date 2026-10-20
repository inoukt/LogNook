# Proposed LogNook source-available policy

**Status:** requirements discussion only. This document is not a license and grants no rights. The repository still has no root `LICENSE` file.

## Intended permissions

The project owner wants people to be able to get LogNook for free, inspect and modify its source, and share copies and modified versions without charge. A person or shop may use LogNook in their work and charge for vehicle repair or diagnostic services. No one may charge for LogNook itself, a rebranded copy, a modified fork, or paid access to substantially the same software.

This is source-available licensing, not an OSI-approved open-source license. The Open Source Definition says a license must not restrict use in a field of endeavor, including business use: <https://opensource.org/osd>.

## Existing terms reviewed

- **Commons Clause** is the closest existing add-on for preventing someone from selling the software itself while retaining commercial use. Its definition bars selling a product or service whose value derives entirely or substantially from the software. It also permits selling products with substantial added value and derivative works, so it does not enforce an absolute ban on selling every modified fork. Its text mentions consulting/support fees, and its FAQ explains that professional service value should come from expertise; the wording still merits careful review. Official text and FAQ: <https://commonsclause.com/>. The clarification discussion: <https://github.com/fossas/commons-clause/issues/4>.
- **PolyForm Noncommercial** allows use, modification, and redistribution for noncommercial purposes. Its business-use restriction would also affect garages using LogNook in paid work, so it does not match the stated goal. Official terms: <https://polyformproject.org/licenses/noncommercial/1.0.0>.
- **GPL-3.0** requires source and same-license terms for covered distributed modifications, but permits commercial use and charging for copies. It would not enforce the no-sale requirement. Summary and terms: <https://choosealicense.com/licenses/gpl-3.0/>.

## Drafting requirements for a final license

Ask a qualified software-licensing lawyer to prepare or review terms that explicitly:

1. Permit free use, inspection, modification, and redistribution of the source and binaries.
2. Permit commercial use of LogNook as a tool in a garage, including charging customers for repair and diagnostic services.
3. Prohibit charging for copies, licenses, hosted access, or any modified version of LogNook, including rebranded forks.
4. Explain how paid support, training, integrations, or hardware bundles are treated without blocking ordinary vehicle-service work.
5. Define the source-sharing obligation for distributed modifications and preserve contributor attribution.

Before applying terms retroactively, verify that the project has the rights needed to license existing contributions. The OEM PID database and other third-party material retain their own license and attribution requirements; see [the catalog licensing guide](../PIDs/LogNook_PID_Master_v0_3/docs/03_SOURCES_LICENSING.md).
