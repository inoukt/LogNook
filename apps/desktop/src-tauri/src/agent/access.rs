use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(unix)]
use std::{fs::OpenOptions, io::Write, os::unix::fs::PermissionsExt, path::Path};

const PIN_LENGTH: usize = 6;
const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const FAILED_ATTEMPTS_BEFORE_LOCKOUT: u8 = 5;
const LOCKOUT_DURATION: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AgentAccessStatus {
    pub configured: bool,
    pub unlocked: bool,
    pub host_connected: bool,
    pub expires_at_unix_ms: Option<u64>,
    pub active_operation: Option<String>,
    pub trace_gaps: u64,
}

pub(crate) struct AgentAccessManager {
    pin_path: PathBuf,
    pin_verifier: Option<String>,
    last_action: Option<Instant>,
    failed_attempts: u8,
    locked_until: Option<Instant>,
}

impl AgentAccessManager {
    pub(crate) fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let pin_path = path.into();
        #[cfg(unix)]
        protect_pin_directory(&pin_path)?;
        let pin_verifier = match fs::read_to_string(&pin_path) {
            Ok(verifier) => {
                #[cfg(unix)]
                protect_pin_file(&pin_path)?;
                let verifier = verifier.trim().to_owned();
                PasswordHash::new(&verifier)
                    .map_err(|_| "Stored Agent Access PIN verifier is invalid".to_owned())?;
                Some(verifier)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("Could not read Agent Access PIN verifier: {error}")),
        };
        Ok(Self {
            pin_path,
            pin_verifier,
            last_action: None,
            failed_attempts: 0,
            locked_until: None,
        })
    }

    pub(crate) fn configure_pin(&mut self, pin: &str) -> Result<(), String> {
        if !valid_pin(pin) {
            return Err(format!("PIN must contain exactly {PIN_LENGTH} digits"));
        }
        if self.pin_verifier.is_some() && !self.status().unlocked {
            return Err("Unlock Agent Access before changing the PIN".into());
        }
        let verifier = Argon2::default()
            .hash_password(pin.as_bytes())
            .map_err(|error| format!("Could not hash Agent Access PIN: {error}"))?
            .to_string();
        if let Some(parent) = self
            .pin_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| {
                format!("Could not create Agent Access data directory: {error}")
            })?;
            #[cfg(unix)]
            protect_pin_directory(&self.pin_path)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&self.pin_path)
                .map_err(|error| format!("Could not save Agent Access PIN verifier: {error}"))?;
            file.write_all(verifier.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("Could not save Agent Access PIN verifier: {error}"))?;
            protect_pin_file(&self.pin_path)?;
        }
        #[cfg(not(unix))]
        fs::write(&self.pin_path, verifier.as_bytes())
            .map_err(|error| format!("Could not save Agent Access PIN verifier: {error}"))?;
        self.pin_verifier = Some(verifier);
        self.lock();
        self.failed_attempts = 0;
        self.locked_until = None;
        Ok(())
    }

    pub(crate) fn unlock(&mut self, pin: &str) -> Result<(), String> {
        self.unlock_at(pin, Instant::now())
    }

    fn unlock_at(&mut self, pin: &str, now: Instant) -> Result<(), String> {
        let Some(verifier) = self.pin_verifier.as_deref() else {
            return Err("Set an Agent Access PIN in Settings first".into());
        };
        if self.locked_until.is_some_and(|until| now < until) {
            return Err("Too many incorrect PIN attempts; wait 30 seconds and try again".into());
        }
        if self.locked_until.is_some_and(|until| now >= until) {
            self.locked_until = None;
            self.failed_attempts = 0;
        }
        let correct = valid_pin(pin)
            && PasswordHash::new(verifier).ok().is_some_and(|hash| {
                Argon2::default()
                    .verify_password(pin.as_bytes(), &hash)
                    .is_ok()
            });
        if !correct {
            self.failed_attempts = self.failed_attempts.saturating_add(1);
            if self.failed_attempts >= FAILED_ATTEMPTS_BEFORE_LOCKOUT {
                self.locked_until = Some(now + LOCKOUT_DURATION);
                self.failed_attempts = 0;
            }
            return Err("Incorrect Agent Access PIN".into());
        }
        self.failed_attempts = 0;
        self.locked_until = None;
        self.last_action = Some(now);
        Ok(())
    }

    pub(crate) fn authorize_action(&mut self) -> Result<(), String> {
        self.authorize_action_at(Instant::now())
    }

    fn authorize_action_at(&mut self, now: Instant) -> Result<(), String> {
        if !self.status_at(now).unlocked {
            return Err("Agent Access is locked; unlock it in LogNook Settings".into());
        }
        self.last_action = Some(now);
        Ok(())
    }

    pub(crate) fn lock(&mut self) {
        self.last_action = None;
    }

    pub(crate) fn status(&mut self) -> AgentAccessStatus {
        self.status_at(Instant::now())
    }

    fn status_at(&mut self, now: Instant) -> AgentAccessStatus {
        let elapsed = self
            .last_action
            .map(|last_action| now.saturating_duration_since(last_action));
        let remaining = elapsed
            .filter(|elapsed| *elapsed < IDLE_TIMEOUT)
            .map(|elapsed| IDLE_TIMEOUT - elapsed);
        if remaining.is_none() {
            self.last_action = None;
        }
        let expires_at_unix_ms = remaining.map(|remaining| {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            now_ms
                .saturating_add(remaining.as_millis())
                .min(u64::MAX as u128) as u64
        });
        AgentAccessStatus {
            configured: self.pin_verifier.is_some(),
            unlocked: remaining.is_some(),
            host_connected: false,
            expires_at_unix_ms,
            active_operation: None,
            trace_gaps: 0,
        }
    }
}

#[cfg(unix)]
fn protect_pin_directory(path: &Path) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        && parent.exists()
    {
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not protect Agent Access data directory: {error}"))?;
    }
    Ok(())
}

#[cfg(unix)]
fn protect_pin_file(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Could not protect Agent Access PIN verifier: {error}"))
}

fn valid_pin(pin: &str) -> bool {
    pin.len() == PIN_LENGTH && pin.bytes().all(|byte| byte.is_ascii_digit())
}
#[cfg(test)]
mod tests {
    use super::{AgentAccessManager, AgentAccessStatus};
    use std::{
        fs,
        path::PathBuf,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    struct TestManager {
        directory: PathBuf,
        manager: AgentAccessManager,
    }

    impl TestManager {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!("lognook-agent-access-{nonce}"));
            fs::create_dir_all(&directory).unwrap();
            let manager = AgentAccessManager::open(directory.join("pin.hash")).unwrap();
            Self { directory, manager }
        }

        fn status_at(&mut self, now: Instant) -> AgentAccessStatus {
            self.manager.status_at(now)
        }
    }

    impl Drop for TestManager {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn agent_access_starts_locked() {
        let mut test = TestManager::new();
        assert!(!test.status_at(Instant::now()).unlocked);
    }

    #[test]
    fn wrong_pin_does_not_unlock() {
        let mut test = TestManager::new();
        test.manager.configure_pin("135790").unwrap();
        assert!(test.manager.unlock_at("000000", Instant::now()).is_err());
        assert!(!test.status_at(Instant::now()).unlocked);
    }

    #[test]
    fn pin_verifier_does_not_contain_pin() {
        let mut test = TestManager::new();
        test.manager.configure_pin("135790").unwrap();
        let verifier = fs::read_to_string(test.directory.join("pin.hash")).unwrap();
        assert!(!verifier.contains("135790"));
        assert!(verifier.starts_with("$argon2id$"));
    }

    #[cfg(unix)]
    #[test]
    fn pin_verifier_and_parent_directory_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let mut test = TestManager::new();
        test.manager.configure_pin("135790").unwrap();
        fs::set_permissions(&test.directory, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(
            test.directory.join("pin.hash"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let _ = AgentAccessManager::open(test.directory.join("pin.hash")).unwrap();
        let dir_mode = fs::metadata(&test.directory).unwrap().permissions().mode() & 0o777;
        let file_mode = fs::metadata(test.directory.join("pin.hash"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }

    #[test]
    fn invalid_pin_format_is_rejected_without_saving_a_verifier() {
        let mut test = TestManager::new();
        assert!(test.manager.configure_pin("12x456").is_err());
        assert!(!test.directory.join("pin.hash").exists());
    }

    #[test]
    fn agent_access_expires_after_fifteen_minutes_without_action() {
        let mut test = TestManager::new();
        let start = Instant::now();
        test.manager.configure_pin("135790").unwrap();
        test.manager.unlock_at("135790", start).unwrap();
        assert!(
            test.status_at(start + Duration::from_secs(14 * 60 + 59))
                .unlocked
        );
        assert!(
            !test
                .status_at(start + Duration::from_secs(15 * 60))
                .unlocked
        );
    }

    #[test]
    fn five_failed_attempts_trigger_a_thirty_second_lockout() {
        let mut test = TestManager::new();
        let start = Instant::now();
        test.manager.configure_pin("135790").unwrap();
        for seconds in 0..5 {
            assert!(
                test.manager
                    .unlock_at("000000", start + Duration::from_secs(seconds))
                    .is_err()
            );
        }
        assert!(
            test.manager
                .unlock_at("135790", start + Duration::from_secs(5))
                .is_err()
        );
        assert!(
            test.manager
                .unlock_at("135790", start + Duration::from_secs(35))
                .is_ok()
        );
    }
}
