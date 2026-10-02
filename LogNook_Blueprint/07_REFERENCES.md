# Primary source register and research limitations
Research checked 2026-10-02. These sources support technical *planning*, not unperformed field tests. Obtain the current edition of paid standards and check redistribution rights before commercial release.

## Hardware, standards, drivers
- Tactrix official OpenPort 2.0 driver/J2534 download: https://www.tactrix.com/index.php?Itemid=61&id=38&option=com_content&view=category
- Tactrix official product and protocol limitations: https://www.tactrix.com/index.php?FontSize=font-large&flypage=flypage.tpl&fontsize=font-large&fontsize=font-medium&itemid=53&option=com_virtuemart&page=shop.product_details&product_id=17&redirected=1
- OBDLink SX specs: https://www.obdlink.com/products/obdlink-sx/
- OBDLink SX official drivers: https://www.obdlink.com/support/sxusb/
- ScanTool developer documentation / ST command guide: https://www.scantool.net/scantool/downloads/product-documentation/
- Original ELM327 specification and limitations: https://www.elmelectronics.com/wp-content/uploads/2017/01/ELM327DS.pdf
- Jhoinrch official RH-02 factory firmware statement: https://jhoinrch.net/index.php?a=index&aid=124&c=View&m=home
- CANable getting started, firmware and Windows/Linux setup: https://www.canable.io/getting-started.html
- candleLight source and device support caveats: https://github.com/candle-usb/candleLight_fw
- python-can documented gs_usb capability/Windows WinUSB: https://python-can.readthedocs.io/en/main/interfaces/gs_usb.html
- SAE J1979-2 (OBDonUDS): https://saemobilus.sae.org/standards/j1979-2_202104-e-e-diagnostic-test-modes-obdonuds (verify superseding 2026 revision)
- SAE J1979DA digital annex: https://saemobilus.sae.org/standards/j1979da_202510-j1979-da-digital-annex-e-e-diagnostic-test-modes (verify superseding 2026 revision)
- ASAM ODX: https://www.asam.net/standards/detail/mcd-2-d/
- ASAM A2L: https://www.asam.net/standards/detail/mcd-2-mc/
- SAE J1939 diagnostics: https://saemobilus.sae.org/standards/j1939-73_202609-application-layer-diagnostics

## App technology and integration
- Tauri framework: https://v2.tauri.app/start/
- Tauri Windows WebView2 installer constraints: https://v2.tauri.app/distribute/windows-installer/
- uPlot time-series performance and MIT license: https://github.com/leeoniya/uPlot
- Apache ECharts options: https://echarts.apache.org/en/feature.html
- `nusb` low-level cross-platform Rust USB: https://docs.rs/crate/nusb/latest
- Out-of-process J2534 implementation reference, not yet audited or adopted: https://github.com/mickeyl/j2534-bridge
- Datazap official product scope: https://datazap.me/
- Google installed-app OAuth: https://developers.google.com/identity/protocols/oauth2/native-app
- Google Drive file upload: https://developers.google.com/workspace/drive/api/guides/manage-uploads
- OpenAI developer quickstart/API tools (verify current production design): https://platform.openai.com/docs/quickstart/make-your-first-api-request

## Explicitly unresolved
No verified public Datazap partner-upload API, signed driver redistribution rights for each hardware family, universal OEM-enhanced PID library, unrestricted security gateway support, benchmarked speeds across the four physical devices, nor compatibility evidence for every CANable-branded clone. Do not advertise any of these as accomplished.

## Existing-product reference workflows
- Datazap online plots, heatmaps, math channels and comparisons: https://datazap.me/
- SavvyCAN raw CAN and DBC/UDS workflows: https://www.savvycan.com/
- RomRaider code, licensing and Subaru-centric scope: https://github.com/RomRaider/RomRaider
- asammdf GUI and measurement exchange formats: https://asammdf.readthedocs.io/en/stable/index.html

## Actual POC vehicles and network caveats (verified against manufacturer/reference material)
- Ford official 2011 Flex owner guide identifying the 3.5L EcoBoost: https://www.fordservicecontent.com/Ford_Content/catalog/owner_guides/11flxog1e.pdf
- Ford 2011 Flex brochure identifying EcoBoost dual-turbocharged direct-injection configuration: https://www.auto-brochures.com/makes/Ford/Flex/Ford_US%20Flex_2011.pdf
- Toyota official 2014 Scion xB product news confirming its 2.4L four-cylinder: https://pressroom.toyota.com/2014-scion-xb-product-news/
- Toyota/Scion service bulletin with a 2014 xB 2AZ-FE Techstream screen: https://static.nhtsa.gov/odi/tsbs/2015/MC-10134472-9999.pdf
- OBDLink official SX specification, specifically advising MX+ for Ford proprietary MS-CAN: https://www.obdlink.com/products/obdlink-sx/
- OBDLink explanation of enhanced diagnostics limitations for adapters without Ford MS-CAN: https://support.obdlink.com/support/solutions/articles/43000713278
- RomRaider's actual Subaru `log_defs.xml` parameter definitions; source/variant validation and licensing still required: https://github.com/RomRaider/RomRaider/blob/master/definitions/log_defs.xml
