use crate::j2534::{J2534Failure, PassThruApi};
use lognook_j2534_protocol::{J2534Identity, Mode01Payload, WorkerErrorKind};

pub(crate) struct J2534Session {
    api: Box<dyn PassThruApi>,
    device: Option<u32>,
    channel: Option<u32>,
}

impl J2534Session {
    pub(crate) fn open(api: Box<dyn PassThruApi>) -> Result<Self, J2534Failure> {
        let mut session = Self {
            api,
            device: None,
            channel: None,
        };
        session.device = Some(session.api.open_device()?);
        Ok(session)
    }

    pub(crate) fn identity(&mut self) -> Result<J2534Identity, J2534Failure> {
        let device = self.device.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::ProviderNotFound,
                "J2534 provider is not open",
            )
        })?;
        let identity = self.api.read_version(device)?;
        if identity.api_version.trim() != "04.04" {
            return Err(J2534Failure::new(
                WorkerErrorKind::UnsupportedApi,
                format!("Expected J2534 API 04.04, got {}", identity.api_version),
            ));
        }
        Ok(identity)
    }

    pub(crate) fn connect(&mut self) -> Result<J2534Identity, J2534Failure> {
        if self.channel.is_some() {
            return Err(J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 channel is already connected",
            ));
        }
        let identity = self.identity()?;
        let device = self.device.expect("identity checked the device");
        let channel = self.api.connect_iso15765(device)?;
        self.channel = Some(channel);
        self.api.start_obd_filter(channel)?;
        Ok(identity)
    }

    pub(crate) fn discover_mode01(&mut self) -> Result<Vec<Mode01Payload>, J2534Failure> {
        let mut responses = Vec::new();
        for pid in [0x00, 0x20, 0x40, 0x60, 0x80, 0xA0, 0xC0, 0xE0] {
            let page = self.read_mode01(pid)?;
            let follows = page.len() == 1
                && page[0].bytes.len() == 6
                && page[0].bytes[0] == 0x41
                && page[0].bytes[1] == pid
                && page[0].bytes[5] & 0x01 != 0;
            responses.extend(page);
            if !follows {
                break;
            }
        }
        Ok(responses)
    }

    pub(crate) fn read_mode01(&mut self, pid: u8) -> Result<Vec<Mode01Payload>, J2534Failure> {
        let channel = self.channel.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::ChannelFailed,
                "J2534 channel is not connected",
            )
        })?;
        self.api.read_mode01(channel, pid, 250)
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(channel) = self.channel.take()
            && let Err(error) = self.api.disconnect(channel)
        {
            eprintln!("OpenPort channel cleanup failed: {error}");
        }
        if let Some(device) = self.device.take()
            && let Err(error) = self.api.close_device(device)
        {
            eprintln!("OpenPort provider cleanup failed: {error}");
        }
    }
}

impl Drop for J2534Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::{J2534Session, PassThruApi};
    use crate::j2534::J2534Failure;
    use lognook_j2534_protocol::{J2534Identity, Mode01Payload, WorkerErrorKind};
    use std::sync::{Arc, Mutex};

    struct FakeApi {
        calls: Arc<Mutex<Vec<String>>>,
        fail_at: Option<&'static str>,
        multiple_page_zero_responders: bool,
    }

    impl FakeApi {
        fn record(&self, call: impl Into<String>) {
            self.calls.lock().unwrap().push(call.into());
        }

        fn fail(&self, point: &'static str) -> Result<(), J2534Failure> {
            if self.fail_at == Some(point) {
                Err(J2534Failure::new(
                    WorkerErrorKind::PassThruFailed,
                    format!("fixture failure at {point}"),
                ))
            } else {
                Ok(())
            }
        }

        fn response(pid: u8, has_more: bool, ecu_id: &str) -> Mode01Payload {
            Mode01Payload {
                ecu_id: ecu_id.into(),
                bytes: vec![0x41, pid, 0x00, 0x00, 0x00, u8::from(has_more)],
                latency_us: 1_000,
            }
        }
    }

    impl PassThruApi for FakeApi {
        fn open_device(&mut self) -> Result<u32, J2534Failure> {
            self.record("open");
            self.fail("open")?;
            Ok(1)
        }

        fn read_version(&mut self, _device: u32) -> Result<J2534Identity, J2534Failure> {
            self.record("version");
            self.fail("version")?;
            Ok(J2534Identity {
                firmware: "fixture".into(),
                dll_version: "1.0".into(),
                api_version: "04.04".into(),
            })
        }

        fn connect_iso15765(&mut self, _device: u32) -> Result<u32, J2534Failure> {
            self.record("connect");
            self.fail("connect")?;
            Ok(2)
        }

        fn start_obd_filter(&mut self, _channel: u32) -> Result<(), J2534Failure> {
            self.record("filter");
            self.fail("filter")
        }

        fn read_mode01(
            &mut self,
            _channel: u32,
            pid: u8,
            _timeout_ms: u32,
        ) -> Result<Vec<Mode01Payload>, J2534Failure> {
            self.record(format!("read:{pid:02X}"));
            self.fail("read")?;
            if pid == 0x00 && self.multiple_page_zero_responders {
                return Ok(vec![
                    Self::response(0x00, true, "7E8"),
                    Self::response(0x00, true, "7E9"),
                ]);
            }
            Ok(vec![Self::response(pid, pid == 0x00, "7E8")])
        }

        fn disconnect(&mut self, _channel: u32) -> Result<(), J2534Failure> {
            self.record("disconnect");
            Ok(())
        }

        fn close_device(&mut self, _device: u32) -> Result<(), J2534Failure> {
            self.record("close");
            Ok(())
        }
    }

    fn fake_api(fail_at: Option<&'static str>) -> (FakeApi, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            FakeApi {
                calls: calls.clone(),
                fail_at,
                multiple_page_zero_responders: false,
            },
            calls,
        )
    }

    #[test]
    fn channel_setup_failure_disconnects_then_closes_the_provider() {
        let (api, calls) = fake_api(Some("filter"));
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        assert!(session.connect().is_err());
        drop(session);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "open",
                "version",
                "connect",
                "filter",
                "disconnect",
                "close"
            ]
        );
    }

    #[test]
    fn channel_connect_failure_closes_the_provider() {
        let (api, calls) = fake_api(Some("connect"));
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        assert!(session.connect().is_err());
        drop(session);
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["open", "version", "connect", "close"]
        );
    }

    #[test]
    fn supported_pid_discovery_follows_only_reported_continuation_pages() {
        let (api, calls) = fake_api(None);
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();
        let responses = session.discover_mode01().unwrap();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].bytes[1], 0x00);
        assert_eq!(responses[1].bytes[1], 0x20);
        let reads: Vec<_> = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("read:"))
            .cloned()
            .collect();
        assert_eq!(reads, vec!["read:00", "read:20"]);
    }

    #[test]
    fn multiple_responders_stop_supported_pid_discovery_at_the_first_page() {
        let (mut api, calls) = fake_api(None);
        api.multiple_page_zero_responders = true;
        let mut session = J2534Session::open(Box::new(api)).unwrap();
        session.connect().unwrap();
        let responses = session.discover_mode01().unwrap();
        assert_eq!(
            responses
                .iter()
                .map(|sample| sample.ecu_id.as_str())
                .collect::<Vec<_>>(),
            vec!["7E8", "7E9"]
        );
        let reads: Vec<_> = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.starts_with("read:"))
            .cloned()
            .collect();
        assert_eq!(reads, vec!["read:00"]);
    }
}
